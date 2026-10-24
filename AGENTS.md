# AGENTS.md — 实现方必读

本项目是端对端加密 (E2EE) 的 Web 备份服务，按生产级标准开发。
**分工：实现由编码 agent 完成；设计与代码审查由另一位审查者完成。** 你的产出会逐组被对照 spec 审查。

## 开工前必须读

1. `openspec/config.yaml` —— 技术栈、架构原则、工程约定（已决策）。
2. `openspec/changes/bootstrap-e2ee-backup-mvp/` 下的 `proposal.md`、`design.md`、`specs/*/spec.md`、`tasks.md`。
3. OpenSpec 实现流程：`.agents/skills/openspec-apply-change/SKILL.md`。

spec 是行为契约，每个 `#### Scenario` 都应有对应测试。design.md 的决策 D1–D16 是约束，不是建议。

## 工作方式

- **一次只做一个任务组**（tasks.md 的 `## N.`）。按编号顺序进行，不跳组、不预先实现后面的组。
- 每组在独立分支上完成：`impl/group-N-<短名>`，从最新 `main` 切出。
- 完成一个任务就在 tasks.md 中把 `- [ ]` 改为 `- [x]`，只勾选**已经通过其验证条件**的任务。
- 一组完成后停止，按下方“交付格式”汇报，等待审查通过并合并后再开始下一组。
- 遇到 spec/design 有歧义、矛盾或你认为有错：**停下来提问**，在汇报中写明，不要自行决定、不要修改 spec/design/proposal。

## 硬性规则（违反任一条审查直接打回）

### 加密
- 不得更改算法、参数默认值、密文帧格式、AAD 前缀字符串、HKDF info 字符串（见 design D4/D5）。它们一旦上线就永久不可变。
- 所有随机数只来自 CSPRNG（Rust `getrandom`/`OsRng`，浏览器 `crypto.getRandomValues`）。禁止 `Math.random`、禁止固定或计数器 nonce（测试向量除外）。
- 密钥比较、令牌比较、哈希比较一律常量时间（`subtle`）。
- 密钥类型实现 `Zeroize`/`ZeroizeOnDrop`；不得为密钥类型派生 `Debug`/`Display`/`Serialize`（或实现为脱敏输出）。
- 解密失败时不得输出任何部分明文。
- 密钥只存在于 Web Worker 内存，不得写入 localStorage/sessionStorage/IndexedDB/Cookie，不得通过 postMessage 传给主线程。

### 服务端
- 非测试代码禁止 `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`（启动期配置校验除外，且需注释说明）。
- 每条访问用户数据的 SQL 必须带 `user_id` 条件；访问他人资源返回 404，与不存在不可区分。
- OSS 对象删除只能写入 `object_deletions` 队列，禁止在请求处理或业务事务中直接删除对象。
- 配额的检查与预留必须在同一事务内并锁定用户行。
- 创建类、提交类接口必须幂等（以客户端生成的 ID 作为幂等键）。
- 日志中不得出现：会话令牌、auth_key、任何密钥或包裹密钥、加密元数据、预签名 URL 的签名部分、请求体原文。
- 只用参数化 SQL（sqlx 宏），禁止字符串拼接 SQL。

### 通用
- 不得引入 design.md 之外的新基础设施组件（Redis、MQ 等）或替换已选库；确有需要先提问。
- 新增依赖需说明理由，且必须通过 `cargo deny check`。
- 不得删除或弱化已有测试来让 CI 通过。
- 不得修改 `openspec/` 目录（唯一例外：勾选 tasks.md 中的复选框）、`AGENTS.md`、`CLAUDE.md`。

## 每组完成前必须全部通过

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
openspec validate bootstrap-e2ee-backup-mvp --strict
```
涉及前端时追加（在 `web/` 下）：`npm run typecheck && npm run lint && npm test && npm run build`。
涉及 WASM 时追加：`wasm-pack test --headless --chrome crates/crypto-wasm`。
涉及数据库查询时追加：`cargo sqlx prepare --workspace --check`。

## 交付格式（每组完成后的汇报）

```
分支：impl/group-N-xxx
完成任务：N.1, N.2, ...（未完成的列出原因）
spec 场景 → 测试映射：<Scenario 名> → <测试函数路径>
上述检查命令的实际输出结论（通过/失败）
与 design 的任何偏离及理由（理想情况为“无”）
待审查者决定的问题
```

## 提交规范

- Conventional Commits：`feat(server): ...`、`feat(crypto): ...`、`test(...)`、`chore(...)`。
- 小步提交，每个提交可编译、测试可通过。
