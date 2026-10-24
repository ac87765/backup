# 加密格式说明（`0x01`）

本文档描述客户端加密的唯一实现（`crates/crypto-core`，经 `crypto-wasm` 在浏览器 Web Worker 中运行）
所采用的密钥层级、密文帧与 AAD 规则。它与 `openspec/changes/bootstrap-e2ee-backup-mvp/design.md`
的 D4/D5/D6 一致；两者冲突时以 design 为准，并且必须先修正文档再改代码。

> **不可变性**：密文格式版本 `0x01` 以及下表中的每个字符串、每种拼接方式，
> 一旦写出第一份真实用户数据就永久不可变。任何改动都必须引入新的格式版本号，
> 并保留对旧版本的解密能力。`docs/crypto-format.md` 与代码常量的漂移由
> `crates/crypto-core/src/format_docs.rs` 中的测试强制检查。

## 1. 标识符与字节表示

节点 ID（`node_id`）、内容 ID（`content_id`）、版本 ID（`version_id`）都是客户端生成的 UUIDv4。
进入 AAD 与 HKDF salt 时统一使用：

```
小写、带连字符的 36 字节 ASCII，例如 550e8400-e29b-41d4-a716-446655440000
```

解析时接受大写、无连字符、花括号与 `urn:uuid:` 形式，但参与密码学计算的一律是上面的规范形式。
nil UUID 与非 v4 UUID（v1/v5 等）一律拒绝：客户端只会生成 v4，出现其他版本说明数据有问题。
`node_id` / `content_id` / `version_id` 同时也是创建类请求的幂等键。

## 2. 密文帧

所有对称加密（MK 包裹、节点密钥包裹、节点/版本元数据、文件块）都使用同一个帧格式：

```
version(1B, 固定 0x01) ‖ nonce(24B) ‖ ciphertext(可变) ‖ tag(16B)
```

| 项 | 值 |
|---|---|
| AEAD 算法 | XChaCha20-Poly1305 |
| nonce | 24 字节，CSPRNG 随机生成，每个 (key, nonce) 只用一次 |
| tag | Poly1305，16 字节 |
| 固定开销 | 41 字节（空明文也是 41 字节密文） |

解密时先检查版本号：不是 `0x01` 时必须报告“需要升级客户端”，不得尝试解密。
任何一字节被改动都会导致认证失败；解密失败绝不输出部分明文。

## 3. 密钥层级

```
password ──Argon2id(NFKC)──▶ root(32B) ──HKDF-SHA256──┬─▶ pwd_wrap  (info: bk/v1/pwd-wrap)
                                                     └─▶ auth_key  (info: bk/v1/auth)

rk(32B, CSPRNG) ──HKDF-SHA256──┬─▶ rk_wrap  (info: bk/v1/rk-wrap)
                               └─▶ rk_auth  (info: bk/v1/rk-auth)

MK(32B, CSPRNG)
 ├─ wrapped_mk          = AEAD(pwd_wrap, MK, aad = "bk/v1/mk")
 ├─ recovery_wrapped_mk = AEAD(rk_wrap,  MK, aad = "bk/v1/mk-recovery")
 ├─ name_key            = HKDF-SHA256(MK, info = "bk/v1/name-token")
 └─ 每个节点 node_key(32B, CSPRNG)
      ├─ wrapped_key    = AEAD(MK, node_key, aad = "bk/v1/node-key|" ‖ node_id)
      ├─ 节点元数据      = AEAD(node_key, json, aad = "bk/v1/node-meta|" ‖ node_id)
      ├─ 版本元数据      = AEAD(node_key, json, aad = "bk/v1/ver-meta|" ‖ version_id)
      └─ content_key    = HKDF-SHA256(node_key, salt = content_id, info = "bk/v1/content")
           └─ chunk_i   = AEAD(content_key, data_i,
                               aad = "bk/v1/chunk|" ‖ content_id ‖ u32be(i) ‖ u32be(n) ‖ u8(is_last))
```

`root` 只作为 HKDF 输入存在，不直接加密任何数据；它和 `MK`、`rk`、各 `node_key` 一样在离开作用域时清零。

## 4. 密码派生

```
root = Argon2id(NFKC(password), salt16, m=64 MiB, t=3, p=1) -> 32B
```

- 密码先做 Unicode **NFKC** 规范化（不同输入法/平台输入同一密码得到同一结果）。
- 参数随账号存储（`users.kdf_params`，prelogin 原样返回），允许每个账号不同。
- 参数 JSON 形状（持久化契约，算法字段必填）：

  ```json
  {"algorithm":"argon2id","m_cost_kib":65536,"t_cost":3,"p_cost":1}
  ```

  `algorithm` 是枚举，当前唯一取值 `argon2id`（隐含 Argon2 版本 `0x13`）；未知值或缺失一律拒绝，
  客户端提示“需要升级客户端”且不发送任何派生值。
- 客户端在派生前 MUST 校验范围：`65536 ≤ m_cost_kib ≤ 262144`（64–256 MiB）、
  `3 ≤ t_cost ≤ 8`、`p_cost = 1`。下界防止被篡改的 prelogin 响应把客户端降级到廉价参数
  （否则截获的 `auth_key` 可被低成本离线爆破）；上界防止客户端被拖入 OOM/长时间计算。
  弱参数只能通过仅测试可见的入口使用，生产 API 没有绕过校验的路径。
- `pwd_wrap = HKDF-SHA256(root, info = "bk/v1/pwd-wrap")`，`auth_key = HKDF-SHA256(root, info = "bk/v1/auth")`。
  两者互相独立：拿到 `auth_key` 无法推出 `pwd_wrap`。
- 服务端存 `auth_hash = HMAC-SHA256(pepper_vN, auth_key)` 与 `pepper_version`，比对使用常量时间。

HKDF 未显式给出 salt 时使用 RFC 5869 的“全零盐”。

## 5. 域分隔常量

| 用途 | 值 |
|---|---|
| 包裹 MK（密码路径）的 AAD | `bk/v1/mk` |
| 包裹 MK（恢复路径）的 AAD | `bk/v1/mk-recovery` |
| 节点密钥包裹 AAD 前缀（后接 node_id） | `bk/v1/node-key|` |
| 节点元数据 AAD 前缀（后接 node_id） | `bk/v1/node-meta|` |
| 版本元数据 AAD 前缀（后接 version_id） | `bk/v1/ver-meta|` |
| 文件块 AAD 前缀 | `bk/v1/chunk|` |
| HKDF info：密码包裹密钥 | `bk/v1/pwd-wrap` |
| HKDF info：认证密钥 | `bk/v1/auth` |
| HKDF info：名称令牌密钥 | `bk/v1/name-token` |
| HKDF info：内容密钥 | `bk/v1/content` |
| HKDF info：恢复包裹密钥 | `bk/v1/rk-wrap` |
| HKDF info：恢复认证值 | `bk/v1/rk-auth` |

文件块 AAD 的完整拼接（按顺序，无分隔符）：

```
"bk/v1/chunk|" ‖ content_id(36B ASCII) ‖ u32be(index) ‖ u32be(count) ‖ u8(is_last)
```

`is_last` 为 `0x01` 表示最后一块，`0x00` 表示不是。

## 6. 元数据

节点元数据用节点密钥加密，明文是一个 JSON 对象：

```json
{"name":"报告 2026.pdf"}
```

版本元数据同样用节点密钥加密，明文是：

```json
{"content_id":"6ba7b811-9dad-41d1-80b4-00c04fd430c8","size":10485760,"mtime":1700000000,"mime":"application/pdf"}
```

- `content_id`：该版本引用的内容 ID（规范小写 UUID 字符串）；`size`：明文总大小（字节，u64）；
  `mtime`：原始修改时间（Unix 秒，i64）；`mime`：MIME 类型，未知时为空串。
- 字段顺序如上（JSON 不要求顺序，但实现按此顺序序列化以便测试向量稳定）。
- **版本与内容绑定**：版本 → content 的指针由服务端保存。下载任一版本时，客户端 MUST 先解密
  版本元数据并校验其 `content_id` 等于服务端给出的 content，再在解密完成后校验明文总长度
  等于 `size`；任一不符即按完整性错误处理且不保存文件。这样服务端把版本指向同一节点的另一个
  （更旧的）content 时会被检测出来（回滚攻击）。
- 恢复旧版本时，客户端为新 `version_id` 重新加密一份版本元数据，其 `content_id` 指向被恢复的 content。
- 服务端永远看不到这两个 JSON 的任何字段。

## 7. 分块

- 明文块大小固定 4 MiB（`4194304` 字节），只有最后一块可以更小。
- 非最后一块必须恰好 4 MiB；空文件是“恰好一块空块”（`count = 1`、`index = 0`、`is_last = 1`、明文 0 字节）。
- 一次上传产生一个 `content`（`content_id` 由客户端生成）。恢复旧版本时新版本引用同一个 `content`，
  不重新加密、不重复计配额。
- `content_key = HKDF-SHA256(node_key, salt = content_id, info = "bk/v1/content")`：
  同一节点密钥下，不同 `content_id` 得到不同内容密钥。
- 解密时 `index` / `count` / `is_last` 必须来自服务端返回的块清单并作为 AAD 传入，
  因此重排、截断（服务端声称更少的块）、跨内容拼接都会认证失败。

## 8. 名称令牌

```
name_token = HMAC-SHA256(name_key, parent_id ‖ NFC(lower(NFC(name))))
```

- `parent_id` 是父节点 UUID 的规范小写字符串（36 字节 ASCII）。
- `name` 先做 Unicode **NFC** 规范化，再转小写，然后**再**做一次 NFC
  （`NFC(lower(NFC(name)))`）：大小写映射可能产生非 NFC 的序列，第二次规范化保证结果唯一。
- 结果是 32 字节，服务端以十六进制文本比较；数据库上有部分唯一索引
  `(user_id, parent_id, name_token) WHERE deleted_at IS NULL`。
- 不同目录下同名文件的令牌不同（`parent_id` 参与 HMAC）；同一目录下大小写不同或 NFC/NFD 不同但语义相同的名称视为重名。

## 9. 恢复密钥

```
rk       = 32 字节 CSPRNG 随机
payload  = rk ‖ SHA-256(rk)[0..2]        # 34 字节
display  = Crockford-Base32(payload)      # 55 个字符，每 5 个一组用 '-' 连接
```

- Crockford Base32 字母表：`0123456789ABCDEFGHJKMNPQRSTVWXYZ`（不含 I、L、O、U）。
- 34 字节 = 272 位，按高位在前的位流编码，最后一组补 3 个 `0` 位，共 55 字符（11 组）。
- 解码要求填充位为 0；输入忽略 `-` 与空白、大小写不敏感、`I`/`L` 读作 `1`、`O` 读作 `0`、`U` 非法。
- 校验位不匹配时客户端直接提示“抄写错误”，不发送任何请求。
- `rk_wrap = HKDF-SHA256(rk, info = "bk/v1/rk-wrap")`，`rk_auth = HKDF-SHA256(rk, info = "bk/v1/rk-auth")`；
  服务端存 `recovery_auth_hash = HMAC-SHA256(pepper, rk_auth)`。

## 10. 测试向量

`testdata/crypto-vectors/` 下是跨实现共用的固定向量：

| 文件 | 覆盖内容 |
|---|---|
| `aead.json` | 帧格式、空明文、Unicode AAD、空 AAD |
| `kdf.json` | 默认参数派生、NFKC 规范化（全角字符）；参数均在允许范围内 |
| `node.json` | 节点密钥包裹、节点/版本元数据（含 `content_id` 绑定与大小校验）、名称令牌（含大小写折叠）、AAD 拼接 |
| `chunk.json` | 空文件单块、普通单块、两块内容的最后一块 |
| `recovery.json` | 恢复密钥展示编码、`rk_wrap`/`rk_auth` 派生 |

固定 nonce 的封帧入口 `aead::seal_with_nonce` 位于 `test-vectors` feature 之后
（crate 自身测试通过 `cfg(test)` 可见）；下游实现若要跑向量，在 dev-dependencies 中开启：

```toml
crypto-core = { path = "...", features = ["test-vectors"] }
```

生产构建不会包含该符号（CI 会检查）。

重新生成向量（仅在明确知道影响时执行，并在 PR 中说明原因）：

```bash
cargo test -p crypto-core -- --ignored regenerate_vectors
```

任何实现（浏览器 WASM、桌面端、CLI）都必须能通过同一组向量；向量变化等同于格式变化。

## 11. 变更流程

1. 新增用途时必须新增域分隔字符串，不能复用已有 AAD/info。
2. 修改帧格式、AAD 拼接、HKDF info、Crockford 编码规则 → 必须新增密文格式版本号。
3. 每次改动都要同步：代码常量、本文档、测试向量、`design.md` 决策，并确保 `cargo test -p crypto-core` 全绿。
