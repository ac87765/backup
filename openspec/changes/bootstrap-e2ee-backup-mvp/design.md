# Design

## Context

全新仓库，无既有代码。动机与范围见 proposal.md；行为契约见 `specs/` 下五个能力。
外部约束：存储必须使用阿里云 OSS；后端必须是 Rust；只做 Web 端；E2EE 意味着所有密码学操作只能在浏览器中进行。

### 总体架构

```
 浏览器 ─────────────────────────────────────────────────────────────┐
 │ React UI (主线程)                                                  │
 │   │  Comlink RPC（只传 ID/句柄与明文给用户展示，不传密钥）            │
 │   ▼                                                                │
 │ Crypto Worker  ── crypto-wasm (Rust→WASM)                          │
 │   持有 MK/节点密钥；读取 File 分片 → 加密 → fetch PUT 预签名 URL      │
 └────┬───────────────────────────────────────────────┬──────────────┘
      │ HTTPS /api/v1 (JSON, 会话 Cookie)               │ HTTPS 预签名 PUT/GET（只有密文）
      ▼                                                ▼
 ┌──────────────────────────┐   OpenDAL (内网 endpoint)  ┌────────────────┐
 │ server (Axum)            │ ─────────────────────────▶ │ 阿里云 OSS      │
 │  api / domain / storage  │  stat / delete / list      │ 私有 bucket     │
 │  jobs (后台任务)          │                            │ SSE-OSS 加密    │
 └──────────┬───────────────┘                            └────────────────┘
            │ sqlx
            ▼
      PostgreSQL（唯一事实来源：账号、会话、节点树、版本、块、上传会话、删除队列）
```

### 仓库结构

```
Cargo.toml                 # workspace
crates/crypto-core/        # 纯 Rust 加密与格式，无 IO，可 no-std 友好；含测试向量
crates/crypto-wasm/        # wasm-bindgen 包装，暴露给 Worker
crates/server/             # Axum 服务：config, api, domain, db, storage, jobs, mail
migrations/                # sqlx 迁移
web/                       # Vite + React + TS；src/worker 内含加密与传输管线
deploy/                    # Dockerfile, docker-compose.dev.yml, OSS 策略/CORS 示例
testdata/crypto-vectors/   # 跨实现测试向量 (JSON)
```

## Goals / Non-Goals

**Goals:**
- 加密格式一开始就带版本号并有测试向量，后续桌面端可零改动复用 `crypto-core`。
- 每个跨系统流程（上传、删除、清理）都有可恢复的持久状态，任一步骤崩溃都不会丢数据或泄露配额。
- 可在多实例下水平扩展：服务无本地状态（除每实例 IP 限流），后台任务可并发安全运行。

**Non-Goals:**
- 不追求对抗“恶意服务端下发篡改的前端代码”——这是 Web E2EE 的固有局限，在风险中说明，留待桌面端/浏览器扩展解决。
- 不实现服务端分块级去重、压缩（压缩在加密前做会引入长度侧信道且收益不确定，MVP 不做）。

## Decisions

### D1. 前端 TS + Rust WASM 加密核心
- **决策**：UI 用 React + TypeScript；加密核心 `crypto-core` 用 Rust 编写，经 `crypto-wasm` 编译为 WASM，运行在专用 Web Worker 中。
- **理由**：加密只有一份实现（浏览器与未来桌面端共用，测试向量一致）；UI 层借用成熟生态（文件选择、拖拽、虚拟列表、路由、请求缓存）。Worker 隔离让密钥不进入主线程，也不会因大文件加密卡住 UI。
- **备选**：Leptos 全 Rust 前端（单语言，但 UI 与上传/下载 Web API 绑定需要大量自研）；纯 Web Crypto（不支持 XChaCha20 与 Argon2，且无法与桌面端共享实现）。
- **风险**：WASM 与 TS 边界的类型同步 → 用 `tsify`/wasm-bindgen 生成 TS 类型，并对 Worker API 写契约测试。

### D2. 后端 Axum + sqlx + PostgreSQL
- **决策**：Axum 0.8 + Tokio；sqlx（编译期校验 SQL、`sqlx migrate`）；PostgreSQL 16（生产建议阿里云 RDS）。
- **理由**：树结构查询（递归 CTE）、部分唯一索引、`FOR UPDATE SKIP LOCKED` 任务领取、事务性配额都是 PostgreSQL 的强项；sqlx 不引入 ORM 抽象，SQL 显式、可审查。
- **备选**：SeaORM/Diesel（更多抽象，复杂查询反而难写）；SQLite（不适合多实例）。
- **风险**：sqlx 离线模式需要提交 `.sqlx/` 查询缓存 → CI 中 `cargo sqlx prepare --check`。

### D3. 存储访问用 OpenDAL，浏览器直传预签名 URL
- **决策**：服务端通过 OpenDAL `services-oss` 访问 OSS，只做 `presign_write`/`presign_read`/`stat`/`delete`/`list`；文件密文由浏览器直接 PUT/GET 到 OSS。服务端配置两个 operator：内网 endpoint 用于服务端自身操作，公网 endpoint 仅用于生成预签名 URL。
- **理由**：文件流量不经过服务端，节省带宽和服务器成本；数据已是密文，直传不降低安全性。OpenDAL 原生支持 OSS 且同一抽象可切换 MinIO(S3) 用于开发测试。
- **备选**：服务端代理上传（实现简单、配额可严格控制，但带宽成本翻倍、成为瓶颈）；阿里云官方 SDK（Rust 版不成熟）；aws-sdk-s3 走 OSS 的 S3 兼容层（兼容层有细节差异）。
- **风险**：预签名 PUT 无法强制对象大小 → 块确认时 `stat` 实际大小并与声明比对，提交时校验总和；不匹配的对象进删除队列。客户端可上传后不确认制造孤儿对象 → 会话过期清理 + 孤儿巡检（D10）。

### D4. 认证：客户端派生认证密钥（Bitwarden 式），而非 OPAQUE
- **决策**：
  ```
  root      = Argon2id(NFKC(password), salt16, m=64MiB, t=3, p=1) → 32B
  pwd_wrap  = HKDF-SHA256(root, info="bk/v1/pwd-wrap")
  auth_key  = HKDF-SHA256(root, info="bk/v1/auth")
  服务端存:  auth_hash = HMAC-SHA256(pepper_vN, auth_key), pepper_version
  ```
  登录比对使用常量时间比较。pepper 来自配置，带版本号以支持轮换（用户下次登录时用新 pepper 重算）。
  KDF 参数的持久化 JSON 契约（`users.kdf_params`，prelogin 原样返回）：
  ```json
  {"algorithm":"argon2id","m_cost_kib":65536,"t_cost":3,"p_cost":1}
  ```
  `algorithm` 是枚举，当前唯一取值 `argon2id`（隐含 Argon2 版本 0x13），未知值或缺失一律拒绝；新增算法只能追加枚举值。
  客户端在派生前 MUST 校验参数范围：`65536 ≤ m_cost_kib ≤ 262144`（64–256 MiB）、`3 ≤ t_cost ≤ 8`、`p_cost = 1`。
  下界防止被篡改的 prelogin 响应把客户端降级到廉价参数（从而让截获的 auth_key 可被低成本离线爆破），上界防止客户端被拖入 OOM/长时间计算。
  测试可使用更弱参数，但只能通过仅测试可见的入口，生产 API 不提供绕过校验的路径。
- **理由**：服务端从不接触密码；auth_key 是高熵值，服务端不需要再做慢哈希——数据库泄露后攻击者本来就可以拿 `wrapped_mk` 做离线猜测，其代价由客户端 Argon2id 决定，服务端再加一层 Argon2 并不提高这条路径的成本，反而增加登录 CPU 开销与 DoS 面。pepper 防止仅数据库泄露时直接用 hash 构造登录。
- **备选**：OPAQUE（`opaque-ke`）——更强（服务端连 auth_key 都看不到、抵御预计算），但协议复杂、需要两轮交互、浏览器侧实现与审计成本高；列为后续 change。SRP——同样复杂且已被 OPAQUE 取代。
- **风险**：KDF 参数在客户端执行，低端手机上 64 MiB/t=3 可能需要 2–4 秒 → 在 Worker 中执行并显示进度；参数随账号存储，可逐步调整。

### D5. 密钥层级与密文格式
- **决策**：
  ```
  MK (32B 随机)
   ├─ wrapped_mk          = AEAD(pwd_wrap,      MK, aad="bk/v1/mk")
   ├─ recovery_wrapped_mk = AEAD(rk_wrap,       MK, aad="bk/v1/mk-recovery")
   ├─ name_key            = HKDF(MK, "bk/v1/name-token")
   └─ 每个节点 node_key (32B 随机)
        ├─ wrapped_key     = AEAD(MK, node_key, aad="bk/v1/node-key|" ‖ node_id)
        ├─ 节点元数据        = AEAD(node_key, json{name}, aad="bk/v1/node-meta|" ‖ node_id)
        ├─ 版本元数据        = AEAD(node_key, json{content_id,size,mtime,mime}, aad="bk/v1/ver-meta|" ‖ version_id)
        └─ content_key     = HKDF(node_key, salt=content_id, info="bk/v1/content")
             └─ chunk_i     = AEAD(content_key, data_i,
                                   aad="bk/v1/chunk|" ‖ content_id ‖ u32be(i) ‖ u32be(n) ‖ u8(is_last))
  恢复密钥 rk (32B 随机): rk_wrap = HKDF(rk,"bk/v1/rk-wrap"); rk_auth = HKDF(rk,"bk/v1/rk-auth")
  服务端存 recovery_auth_hash = HMAC-SHA256(pepper, rk_auth)
  ```
  AEAD = XChaCha20-Poly1305；统一密文帧：`version(1B)=0x01 ‖ nonce(24B) ‖ ciphertext ‖ tag(16B)`，块密文开销 41 字节。
  恢复密钥展示：`rk ‖ SHA-256(rk)[0..2]` 用 Crockford Base32 编码，按 5 字符分组。
- **理由**：
  - 每节点独立密钥，为将来“分享某个文件夹/文件”留出空间（只需对节点密钥重新包裹），且一个节点密钥泄露不影响其他节点。
  - 每次上传产生一个 `content`（ID 由客户端生成），块 AAD 绑定 content_id 而不是 version_id：恢复旧版本时新版本直接引用原 content，无需重新加密、也不重复计配额。
  - 版本 → content 的指向由服务端保存，若不加保护，服务端可把某版本指向同一节点的另一个（例如更旧的）content，客户端能正常解密却拿到错误内容（回滚攻击）。因此版本元数据明文中包含 `content_id`（规范小写 UUID 字符串）与明文 `size`：下载时客户端 MUST 先解密版本元数据，校验其 `content_id` 等于服务端给出的 content，并在解密完成后校验明文总长度等于 `size`，任一不符即按完整性错误处理。恢复旧版本时，客户端为新 version_id 重新加密一份版本元数据，其 `content_id` 指向被恢复的 content。版本元数据 JSON 契约：`{"content_id":"<uuid>","size":<u64>,"mtime":<i64>,"mime":"<string>"}`。
  - 所有 AAD 带域分隔前缀，杜绝跨用途复用密文。XChaCha 的 192 位随机 nonce 使随机 nonce 碰撞概率可忽略。
  - 节点 ID、content ID、version ID 由客户端生成（UUIDv4），因此客户端加密前就知道 AAD；同时它们天然充当创建类请求的幂等键。服务端校验格式与唯一性。
- **备选**：AES-256-GCM（浏览器 Web Crypto 原生支持、速度更快，但 96 位 nonce 随机生成在大量块下有碰撞上限）；层级包裹（子节点密钥由父文件夹密钥包裹，移动节点需要重新包裹，复杂度高，MVP 不需要）。
- **威胁模型影响**：数据库 + OSS 完全泄露时，攻击者只能对每个账号的密码做离线 Argon2id 猜测；服务端篡改密文/调换密钥/重排块/把版本指向错误 content 均被检出。服务端仍可观察树结构、密文大小、时间（见 client-encryption spec 的信息边界）。

### D6. 同名约束靠名称令牌 + 部分唯一索引
- **决策**：`name_token = HMAC-SHA256(name_key, parent_id ‖ lower(NFC(name)))`；数据库 `UNIQUE (user_id, parent_id, name_token) WHERE deleted_at IS NULL`。
- **理由**：服务端不知道名称也能原子地保证“同一文件夹内不重名”，并发创建由数据库约束兜底，而不是靠客户端先查再写。
- **备选**：允许重名、客户端自行展示（会造成下载/恢复时的歧义）。
- **风险/威胁**：令牌在同一父目录下对同名是确定性的，服务端能看出“某用户在两个时刻在同一目录创建了同名文件”，泄露极小，可接受。移动/恢复到新父目录时客户端必须重算令牌（已写入 spec）。

### D7. 会话：服务端会话 + `__Host-` Cookie
- **决策**：256 位随机令牌，`__Host-session` Cookie（HttpOnly, Secure, SameSite=Strict, Path=/），库中存 SHA-256(token)；空闲 7 天/绝对 30 天；`last_seen_at` 最多每 5 分钟写一次以减轻写压力。改状态请求要求 `X-Requested-With: backup-web`。
- **理由**：可即时撤销（改密码、恢复、登出所有设备）；Cookie 不暴露给 JS，降低 XSS 窃取会话的风险。
- **备选**：JWT（无法即时撤销，需要黑名单，复杂度更高且无收益）。

### D8. 上传会话状态机
```
                create(幂等: content_id)
                       │ 预留配额
                       ▼
   abort ◀────────── open ──────────▶ expired (后台, 超过 24h)
 (释放配额,           │  ▲               (释放配额, 对象入删除队列)
  对象入删除队列)      │  │ presign / confirm chunk（可重复、幂等）
                       ▼
                    commit (单事务)
                       │ 校验全部块已确认、大小总和 == 声明值、目标可用、名称令牌唯一
                       │ 写 content/chunks/file_version，更新 current_version
                       │ reserved_bytes -= n; used_bytes += n
                       ▼
                   committed
```
- 会话声明 `chunk_count` 与 `total_ciphertext_size`；每块确认时 `stat` 对象，要求 `size ≤ 4 MiB + 41` 且等于客户端声明值；提交时要求已确认块大小之和等于声明总量。
- 块对象键：`v1/{user_id}/{object_uuid}`，由服务端在 presign 时生成并写入 `upload_chunks`；同一块重新 presign 会生成新键，旧键（若已上传）在提交/中止时一并清理。
- **理由**：显式状态 + 单向转换让每个崩溃点都有明确的恢复语义；配额在创建时预留，彻底避免并发超额。
- **备选**：OSS Multipart Upload（单个大对象，下载按 Range 读取）——会让每文件只有一个对象、请求更少，但分片在加密前就要定长、续传需要追踪 OSS 的 UploadId 与 ETag 列表、未完成的 multipart 需额外生命周期清理；独立对象方案语义更简单，MVP 选后者，块大小 4 MiB 下请求数可接受。

### D9. 数据模型（核心表）
| 表 | 关键列 | 说明 |
|---|---|---|
| `users` | email (citext unique), email_verified_at, kdf_params jsonb, kdf_salt, auth_hash, pepper_version, wrapped_mk, recovery_wrapped_mk, recovery_auth_hash, quota_bytes, used_bytes, reserved_bytes, failed_logins, locked_until, root_node_id | 计数器与配额在同一行，`SELECT … FOR UPDATE` 保证原子 |
| `email_tokens` | user_id, purpose, token_hash, expires_at, used_at | 验证邮件 |
| `sessions` | user_id, token_hash unique, created_at, last_seen_at, expires_at, revoked_at, ip, user_agent | |
| `nodes` | id (客户端生成), user_id, parent_id, kind, wrapped_key, encrypted_meta, name_token, current_version_id, row_version, deleted_at, trash_batch_id, trashed_from_parent_id | 部分唯一索引见 D6；所有查询都带 `user_id` 条件 |
| `contents` | id (客户端生成), user_id, node_id, chunk_count, ciphertext_size | 计配额的单位 |
| `chunks` | (content_id, idx) PK, object_key, ciphertext_size, sha256 | |
| `file_versions` | id (客户端生成), node_id, content_id, encrypted_meta, created_at | 恢复版本 = 新行指向旧 content |
| `upload_sessions` | id, user_id, content_id unique, target_node_id / parent_id, status, chunk_count, declared_size, reserved_bytes, expires_at, 待提交的加密元数据与包裹密钥 | |
| `upload_chunks` | (session_id, idx, object_key), declared_size, sha256, confirmed_at | |
| `object_deletions` | id, object_key, not_before, attempts, last_error, claimed_until | 删除 outbox |
| `email_outbox` | id, to_email, template, params jsonb, not_before, attempts, last_error, claimed_until | 邮件 outbox（D13） |

- `used_bytes` 在提交/删除 content 的同一事务中增减；另有每日对账任务重新统计并在偏差时打指标告警（不自动修正，避免掩盖 bug）。
- 回收站：删除文件夹时以递归 CTE 给整棵子树打上同一 `trash_batch_id` 与 `deleted_at`；恢复按 batch 还原；永久删除按 batch 删除节点 → 版本 → 不再被引用的 content → chunks，并把 object_key 写入 `object_deletions`，全部在一个事务内。

### D10. 后台任务框架
- **决策**：服务进程内运行 `jobs` 模块（可用配置关闭，以便将来拆分为独立 worker 进程）。两类任务：
  - **队列型**（对象删除）：`UPDATE … SET claimed_until = now()+5min WHERE id IN (SELECT id … WHERE claimed_until < now() AND not_before <= now() ORDER BY id FOR UPDATE SKIP LOCKED LIMIT 100) RETURNING …`；成功则删行，失败则 `attempts+1` 并按指数退避设置 `not_before`。OSS 删除不存在的对象视为成功（幂等）。
  - **扫描型**（会话过期、版本清理、回收站 30 天清理、过期 session/token 清理、孤儿巡检、配额对账）：定时运行，每次运行先 `pg_try_advisory_xact_lock(job_id)`，拿不到就跳过；每批处理有限行数、各自独立事务。
- **理由**：不引入 Redis/MQ 等额外组件；PostgreSQL 已提供所需的并发原语。
- **备选**：独立消息队列（运维成本高）；`apalis` 等任务库（可行，但我们的任务形态简单，自研更透明）。
- **风险**：孤儿巡检需要 list 整个前缀 → 按 `v1/{user_id}/` 分片、分批增量进行，并只处理早于 48 小时的对象，避免与进行中的上传竞争。

### D11. 下载与大文件
- **决策**：浏览器支持 File System Access API 时用 `showSaveFilePicker` 获得可写流；否则使用自托管 Service Worker 流式下载（StreamSaver 模式，不依赖第三方域）。Worker 以 3 并发预取块、按序解密写出。任何块失败 → 中止并调用 `abort()` 丢弃部分文件。
- **理由**：满足“内存与文件大小无关”的 spec。
- **风险**：Safari 对 SW 流式下载的支持较弱 → 对不支持的浏览器限制单文件下载大小（如 2 GiB）并提示，记录在已知限制中。

### D12. 前端技术细节
- 路由 React Router；服务端状态 TanStack Query；API 客户端由 OpenAPI 生成（`openapi-typescript` + `openapi-fetch`），CI 检查生成物与后端一致。
- Worker RPC 用 Comlink；Worker 对主线程只暴露高层操作（`unlock`, `listFolder`, `upload(file, parentId)`, `download(versionId)` …），主线程永远拿不到密钥。
- 上传队列状态持久化到 IndexedDB（只存会话 ID、文件指纹、已确认块数，不存任何密钥或明文）。
- 密码强度用 `@zxcvbn-ts`；UI 组件库选择无运行时 CSS-in-JS 的方案（满足严格 CSP），如 Tailwind + Radix。

#### D12a. 认证类请求由 Worker 发出（第 3 组审查后补充）
- **决策**：prelogin、注册、登录、刷新后解锁（取 KDF 参数与 `wrapped_mk`）、修改密码、恢复密钥重置、轮换恢复密钥这些请求，全部由 Worker 内的认证客户端（AuthClient）用同源 `fetch`（`credentials: 'same-origin'`，带 `X-Requested-With` 头）发出。主线程通过 Comlink 调用高层方法（如 `register(email, password)`、`login(email, password)`、`unlock(password)`、`changePassword(old, new)`、`resetWithRecovery(email, recoveryKey, newPassword)`、`rotateRecoveryKey()`），只拿到成功/失败与错误码。
- **边界**：`auth_key`、`rk_auth` 及其新旧值只存在于 WASM 与 Worker 内；导出为 `Uint8Array`（不使用 JS 普通数组）以便请求体构造后 `fill(0)`。唯一允许传到主线程的密钥类数据是**恢复密钥展示串**：仅用于显示与回填确认，主线程不得持久化或记录日志，确认后立即从组件状态中清除。用户输入的恢复密钥由主线程传入 Worker（入方向）是允许的。
- **理由**：`auth_key` 相当于长期有效的登录凭据，留在 Worker 内可避免主线程 XSS 直接窃取后绕开 HttpOnly 会话长期登录；Worker 同源请求会自动携带并接收会话 Cookie，没有技术障碍。
- **备选**：允许 `auth_key` 经 Comlink 回到主线程由主线程发请求（实现简单，但扩大了凭据暴露面）。
- **风险**：JSON 请求体字符串无法清零 → 接受，生命周期限定在单次请求内。

#### D12b. 改变认证材料的操作采用“准备 → 提交 → 确认/丢弃”
- **决策**：任何会改变服务端认证材料或 Worker 密钥状态的操作，分三步：
  1. **prepare**：在 WASM 内生成待提交材料，保存为 Worker 内唯一的“待提交对象”，**不修改** KeyStore 的现有状态（MK、`pwd_wrap`）；
  2. **submit**：Worker 发请求；
  3. **commit / discard**：服务端返回成功后才把新状态写入 KeyStore（如替换 `pwd_wrap`）；失败、取消或 `lock()` 时丢弃并清零待提交对象。
- **各操作的待提交材料**：
  - 注册：新 MK、按推荐参数派生的 `auth_key` 与 `wrapped_mk`、恢复材料（展示串、`recovery_wrapped_mk`、`rk_auth`）。展示串先交给主线程，用户回填确认后才提交注册。
  - 修改密码：用当前参数与盐派生的旧 `auth_key`（证明知道旧密码），用新随机盐与推荐参数派生的新 `auth_key`，以及用新 `pwd_wrap` 重新包裹的**当前** MK。成功后才替换 `pwd_wrap`。
  - 恢复密钥重置：用输入的恢复密钥解包出 MK（只放在待提交对象中，不进入 KeyStore）、`rk_auth`、新密码派生的 `auth_key` 与 `wrapped_mk`。成功后丢弃待提交对象，由用户用新密码重新登录（服务端已撤销全部会话）。
  - 轮换恢复密钥：新恢复材料，以及用当前参数与盐派生的当前 `auth_key`（证明知道当前密码，第 6 组审查后补充：防止遗留会话静默替换恢复材料）；展示串交给主线程，用户确认后提交，成功后旧恢复密钥失效。
- **理由**：避免“客户端已换成新密钥、服务端却拒绝了请求”导致的状态不一致；中途失败不留下半更新的状态。
- **约束**：新密码一律使用新随机盐与当前推荐 KDF 参数（同时完成参数升级）。待提交对象同一时刻每类最多一个，新的 prepare 会丢弃旧的。

### D13. 邮件
- **决策**：`lettre` 通过 SMTP 发送（生产建议阿里云邮件推送 DirectMail 的 SMTP 接口）；开发环境配置为把邮件写成本地目录中的文件（不写日志，邮件里有一次性令牌）。发送失败不影响注册事务：注册事务写入 `email_outbox`，由队列型后台任务发送并重试。
- **令牌在发送时生成**（第 5 组开工前补充）：`email_outbox` 只保存收件用户、邮件用途与非敏感参数，**不保存任何令牌明文**。发送任务在发送前生成新令牌、把其哈希写入 `email_tokens`，再发信；重试时重新生成（旧的未用令牌作废或自然过期）。这样数据库在任何时刻都只有令牌哈希，满足 user-accounts spec “只保存其哈希”的要求。
- **理由**：与对象删除同一可靠性模式；SMTP 是最通用的接口，切换供应商只改配置。

### D14. 限流
- **决策**：IP 维度用进程内令牌桶（`governor`），依赖反向代理传入的可信 `X-Forwarded-For`（仅信任配置的代理网段）；账号维度失败计数与锁定持久化在 `users` 表。
- **备选**：Redis 集中限流（多实例下更精确，但多一个组件）。MVP 接受“多实例下 IP 限额 ×N”的近似。
- **风险**：攻击者可故意让受害者账号被锁 15 分钟 → 锁定只针对登录，且已有会话不受影响；之后可升级为“锁定后要求验证码/邮件链接”。

### D15. 部署与 OSS 配置
- 单一 Docker 镜像（cargo-chef 缓存 → wasm-pack 构建 → vite 构建 → `debian:bookworm-slim` 运行时，非 root 用户），Axum 以带内容哈希的文件名托管前端静态资源（长缓存），`index.html` 不缓存。
- 迁移：`server migrate` 子命令在发布流程中先执行；`server serve` 启动时只校验版本。
- OSS：私有 bucket；默认服务端加密 SSE-OSS(AES256) 作为纵深防御；**不开**bucket 版本控制（版本由我们管理，否则删除不释放空间）；CORS 只允许前端源的 PUT/GET 并暴露 `ETag`；RAM 子账号最小权限策略只授予该 bucket 前缀 `v1/*` 的 `PutObject/GetObject/DeleteObject/ListObjects/GetObjectMeta`。示例策略放在 `deploy/`。
- 配置全部来自环境变量（`figment`），密钥类配置用 `secrecy::SecretString` 包装，Debug 输出自动脱敏。

### D16. 测试策略
- `crypto-core`：固定测试向量（`testdata/crypto-vectors/*.json`）、篡改/重排/截断负例、`proptest` 往返测试；`crypto-wasm` 在 `wasm-pack test --headless` 下跑同一组向量。
- `server`：集成测试用 testcontainers 启动 PostgreSQL + MinIO，覆盖每个 spec 场景（并发配额、幂等提交、删除队列重试、任务领取不重复）。故障注入：OpenDAL 层包一层可配置失败的 mock layer。
- `web`：Vitest 单元测试；Playwright 端到端（注册→验证→上传→刷新续传→下载比对哈希→改密码→恢复密钥重置）。
- 夜间任务：对真实 OSS 测试 bucket 跑上传/下载冒烟测试，验证预签名与 CORS 配置。
- CI：`cargo fmt --check`、`clippy -D warnings`、`cargo test`、`cargo deny check`（许可证与漏洞）、`cargo sqlx prepare --check`、前端 `tsc --noEmit`/lint/test、OpenAPI 生成物一致性检查。

## Risks / Trade-offs

- [Web E2EE 依赖服务端下发的 JS] → 严格 CSP、SRI、资源哈希命名；发布构建可复现并公开前端构建哈希；在隐私页如实说明；长期方案为桌面端或浏览器扩展。
- [密码 + 恢复密钥同时丢失即数据永久丢失] → 注册强制回填确认、设置页可随时重新生成恢复密钥、定期提醒。
- [客户端 KDF 在弱设备上慢] → Worker 中执行、显示进度；参数按账号存储可调。
- [服务端可见元数据（树结构、大小、时间）] → 在隐私页列明；后续可做大小填充（padding）。
- [预签名直传无法强制大小] → stat 校验 + 提交总量校验 + 孤儿巡检，超额行为最多造成短暂（≤48h）的存储浪费，不影响配额正确性。
- [多实例下 IP 限流不精确] → 前置反向代理/WAF 做全局限流；必要时引入 Redis。
- [配额计数器漂移] → 计数在同事务维护 + 每日对账告警。
- [Safari 大文件下载] → 已知限制，按浏览器能力降级并提示。

## Migration Plan

全新系统，无数据迁移。上线步骤：
1. 创建 RDS PostgreSQL、OSS bucket（私有、SSE、CORS）、RAM 子账号与最小权限策略、SMTP 凭据。
2. 运行 `server migrate`，再部署 `server serve`（≥2 实例，置于负载均衡后，`/readyz` 作为就绪探针）。
3. 回滚：重新部署上一版本镜像；迁移只做向前兼容的增量变更，因此旧版本可在新 schema 上运行。
4. 加密格式版本 `0x01` 一旦上线即永久支持解密；未来新格式只能新增版本号。

## Open Questions

- 默认配额大小、正式域名与 OSS 地域（只影响配置，不影响设计）。
- 邮件供应商最终选择（DirectMail 或其他 SMTP 服务，只影响配置）。
