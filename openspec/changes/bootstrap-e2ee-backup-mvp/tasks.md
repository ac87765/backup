# Tasks

## 1. 工程脚手架

- [x] 1.1 建立 Cargo workspace（`crates/crypto-core`、`crates/crypto-wasm`、`crates/server`），edition 2024、统一 lints（`unwrap_used`/`expect_used` deny），验证 `cargo build --workspace` 与 `cargo clippy --workspace -- -D warnings` 通过
- [x] 1.2 建立 `web/`（Vite + React + TS strict + ESLint + Vitest + Tailwind），验证 `npm run build`、`npm run lint`、`npm test` 通过
- [x] 1.3 编写 `deploy/docker-compose.dev.yml`（PostgreSQL 16 + MinIO + 自动建 bucket 与 CORS），验证 `docker compose up` 后两服务健康
- [x] 1.4 配置 CI：fmt、clippy、test、`cargo deny check`、前端 typecheck/lint/test、wasm 构建，验证在空实现上流水线全绿
- [x] 1.5 添加 `.gitignore`、`rust-toolchain.toml`、`deny.toml`、`README.md`（本地开发启动步骤），按 README 从零启动验证可行
- [x] 1.6 审查修复：CI 增加 `openspec validate --strict` 步骤；dev compose 的 MinIO 与 mc 镜像按 digest 固定；推送到私有 GitHub 远端并让 CI 三个 job 实际跑绿（补齐 1.4 的验证条件），在汇报中附 CI 运行链接

## 2. 加密核心 crypto-core

- [x] 2.1 实现统一 AEAD 帧（version‖nonce‖ct‖tag，XChaCha20-Poly1305）与错误类型，单元测试覆盖篡改任一字节、未知版本号
- [x] 2.2 实现密码派生（NFKC + Argon2id + HKDF 分出 pwd_wrap/auth_key）与 KDF 参数结构，测试两派生值独立且与测试向量一致
- [x] 2.3 实现 MK 生成/包裹/解包、恢复密钥生成与 Crockford Base32+校验位编解码、rk_wrap/rk_auth 派生，测试抄错一位能被校验位检出
- [x] 2.4 实现节点密钥包裹（AAD 绑定 node_id）、节点/版本元数据加解密、name_token 计算，测试调换 node_id 解包失败、大小写与 NFC 变体令牌相同
- [x] 2.5 实现 content_key 派生与分块加解密（AAD 绑定 content_id/idx/count/is_last），测试重排、截断、跨 content 拼接、空文件均按 spec 处理
- [x] 2.6 生成并提交 `testdata/crypto-vectors/*.json`，增加 proptest 往返测试，所有密钥类型实现 `Zeroize`，验证 `cargo test -p crypto-core` 通过
- [x] 2.7 编写 `docs/crypto-format.md` 描述密钥层级与帧格式（与 design D5 一致），验证文档中每个 AAD 前缀与代码常量一致（测试中断言）
- [x] 2.8 KdfParams 增加 `algorithm` 枚举字段（仅 `argon2id`，未知/缺失拒绝），范围收紧为 m 64–256 MiB、t 3–8、p=1，测试用弱参数只走仅测试可见入口；更新 kdf 向量与 docs，测试覆盖降级/过大/未知算法三个 spec 场景
- [x] 2.9 VersionMetadata 增加 `content_id` 字段（JSON 契约见 design D5），提供“解密版本元数据并校验 content_id 与明文大小”的 API，更新 node 向量与 docs，测试覆盖“版本被指向旧内容”“明文大小不符”场景
- [x] 2.10 `aead::seal_with_nonce` 移到 `test-vectors` feature 后（仅 dev-dependencies 开启），验证不开 feature 时该符号不可见、`cargo test --workspace` 仍通过
- [x] 2.11 格式冻结前的收尾：名称规范化改为 `NFC(lower(NFC(name)))`（更新 name_token 向量）；ID 解析拒绝 nil 与非 v4 UUID；`decrypt_chunk` 返回 `Zeroizing<Vec<u8>>` 不再额外复制，各项有对应测试

## 3. 加密 WASM 绑定与 Worker 基座

- [ ] 3.1 实现 `crypto-wasm`（wasm-bindgen + tsify 生成 TS 类型），getrandom 使用浏览器 CSPRNG，验证 `wasm-pack test --headless --chrome` 跑通全部测试向量
- [ ] 3.2 在 `web/src/worker` 建立 Crypto Worker（Comlink），实现密钥仅驻留 Worker 内存的 KeyStore 与 `lock()` 清零逻辑，Vitest 验证主线程 API 不返回任何密钥字节
- [ ] 3.3 验证生产构建下 WASM 在 CSP `script-src 'self' 'wasm-unsafe-eval'` 下可加载（Playwright 冒烟用例）

## 4. 服务端基础设施

- [ ] 4.1 实现配置加载与校验（figment + secrecy），缺项时非零退出并指出项名，单元测试覆盖缺项与非法值
- [ ] 4.2 实现 Axum 应用骨架：请求 ID、JSON tracing 日志（含脱敏字段过滤）、统一错误类型 `{code,message,request_id}`、请求体上限、安全响应头/CSP、静态文件托管，集成测试验证 413、500 不泄露细节、响应头齐全
- [ ] 4.3 接入 sqlx 连接池与 `server migrate` / `server serve` 子命令，serve 时迁移版本落后即退出，集成测试验证
- [ ] 4.4 实现 `/healthz`、`/readyz`（DB + 迁移版本 + 存储 stat）与独立端口 Prometheus 指标，集成测试验证 DB 停止时 readyz=503
- [ ] 4.5 实现存储层：OpenDAL operator（OSS 内网 + 公网 presign 双 operator；dev/test 用 S3/MinIO），封装 presign_write/presign_read/stat/delete/list 与故障注入 layer，集成测试（MinIO）验证预签名 PUT/GET 往返
- [ ] 4.6 实现优雅停机（CancellationToken + TaskTracker，30s 上限），集成测试验证 SIGTERM 时进行中请求完成
- [ ] 4.7 建立 utoipa OpenAPI 生成与 `web/` 端 `openapi-typescript` 生成脚本，CI 检查生成物无漂移
- [ ] 4.8 建立集成测试框架（testcontainers PostgreSQL + MinIO、每测试独立 schema/bucket 前缀），验证可并行运行

## 5. 后台任务框架

- [ ] 5.1 实现队列型任务领取（SKIP LOCKED + claimed_until + 指数退避 not_before）与 `object_deletions` 处理器，集成测试验证双 worker 不重复领取、崩溃后超时重领、删除不存在对象视为成功
- [ ] 5.2 实现扫描型任务调度（advisory lock 互斥、分批、独立事务）与任务指标（执行/失败次数、队列积压与最老年龄），集成测试验证两个实例并发只有一个执行
- [ ] 5.3 实现 `email_outbox` 与 lettre SMTP 发送器（dev 模式写日志），集成测试验证 SMTP 失败后重试成功

## 6. 用户账号

- [ ] 6.1 迁移：`users`、`email_tokens`、`sessions`，含 citext 邮箱唯一与必要索引，验证迁移可在空库执行
- [ ] 6.2 实现 `POST /auth/prelogin`（真实账号返回 KDF 参数；不存在时 HMAC 伪造确定性盐），集成测试验证两次查询一致且响应结构相同
- [ ] 6.3 实现注册：拒绝 `password` 字段、校验包裹材料格式、创建用户与根节点、写验证邮件 outbox；已存在邮箱返回相同响应并发“已有账号”邮件，集成测试覆盖
- [ ] 6.4 实现邮箱验证与重发（令牌哈希、24h、单次），集成测试覆盖过期、重复使用
- [ ] 6.5 实现登录（HMAC+pepper 常量时间比对、pepper 版本升级）、会话 Cookie、失败计数与 15 分钟锁定，集成测试覆盖错误凭据统一响应与锁定
- [ ] 6.6 实现会话中间件（空闲/绝对过期、last_seen 节流、CSRF 头校验）、登出、会话列表与撤销，集成测试覆盖撤销后 401、缺头 403
- [ ] 6.7 实现修改密码、恢复密钥重置、重新生成恢复密钥（原子替换 + 撤销其他会话 + 通知邮件），集成测试覆盖旧凭据错误时数据不变
- [ ] 6.8 实现 IP 维度限流（governor + 可信代理网段解析），集成测试验证 429 与 Retry-After
- [ ] 6.9 前端：注册（强度校验、恢复密钥展示与回填确认）、验证邮件落地页、登录、解锁、登出、会话管理、修改密码、恢复重置页面，Playwright 覆盖注册→验证→登录→刷新解锁→改密→恢复密钥重置全流程

## 7. 文件管理

- [ ] 7.1 迁移：`nodes`、`contents`、`chunks`、`file_versions`，含部分唯一索引 `(user_id,parent_id,name_token) WHERE deleted_at IS NULL`，验证迁移执行
- [ ] 7.2 实现列目录（游标分页）、创建文件夹（客户端 ID 幂等）、重命名（row_version 乐观锁），集成测试覆盖 5000 子项分页、并发重命名冲突、重名冲突
- [ ] 7.3 实现移动（防止移入自身后代、重名检测），集成测试覆盖 `invalid_move` 与 `name_conflict`
- [ ] 7.4 实现回收站：递归软删除（trash_batch_id）、列表、恢复（原位置不可用时报错并支持指定新父节点）、永久删除（删节点→版本→无引用 content→chunks→写 object_deletions，同事务扣减 used_bytes），集成测试覆盖
- [ ] 7.5 实现版本列表、恢复旧版本（新版本引用原 content，不重复计配额），集成测试验证用量不变
- [ ] 7.6 实现扫描任务：版本保留清理（最近 10 个 + 30 天）、回收站 30 天清理、配额对账（偏差只告警），集成测试用可控时钟验证
- [ ] 7.7 实现 `/me/usage`（已用/预留/总额）与跨用户访问一律 404 的仓储层约束，集成测试覆盖猜测他人节点 ID
- [ ] 7.8 前端：文件浏览器（Worker 解密名称后本地排序、虚拟列表）、新建/重命名/移动/删除、回收站、版本历史、用量显示，Playwright 验证数据库中不出现明文文件名

## 8. 文件传输

- [ ] 8.1 迁移：`upload_sessions`、`upload_chunks`、`object_deletions`、`email_outbox`（若未建），验证迁移执行
- [ ] 8.2 实现创建上传会话（content_id 幂等、事务内配额预留、邮箱未验证拒绝），集成测试覆盖并发超额只成功一个
- [ ] 8.3 实现块 presign（服务端生成对象键、≤15 分钟）、块确认（stat 校验大小、幂等）、已确认块查询，集成测试覆盖大小不符、对象不存在、已关闭会话
- [ ] 8.4 实现提交（单事务校验与写入、幂等返回同一结果、目标不可用保持 open）与中止，集成测试覆盖缺块、重复提交、目标被删
- [ ] 8.5 实现会话过期扫描与孤儿对象巡检任务（仅处理 >48h 对象），集成测试验证配额释放与对象入删除队列
- [ ] 8.6 实现下载接口（块清单 + 预签名 GET），集成测试验证他人文件 404
- [ ] 8.7 前端 Worker 上传管线：分片读取→加密→并发 PUT（3–4）→确认→提交，带抖动指数退避（单块 5 次）、IndexedDB 持久化进度与文件指纹、刷新后续传，Vitest + 故障注入验证
- [ ] 8.8 前端 Worker 下载管线：File System Access API 优先、自托管 Service Worker 流式降级、先解密版本元数据并校验 content_id、按序解密、结束时校验明文总大小、失败时丢弃部分文件，Playwright 验证下载文件 SHA-256 与原文件一致
- [ ] 8.9 前端上传/下载队列 UI（进度、暂停/取消、失败重试），Playwright 覆盖断网重连后完成上传

## 9. 部署与端到端验证

- [ ] 9.1 编写多阶段 Dockerfile（cargo-chef、wasm-pack、vite、非 root slim 运行时），验证镜像启动后 `/readyz` 就绪且能访问前端
- [ ] 9.2 在 `deploy/` 提供 OSS RAM 最小权限策略、CORS 配置、SSE 设置说明与生产环境变量清单，并按清单在测试 bucket 上配置一遍验证
- [ ] 9.3 建立对真实 OSS 测试 bucket 的夜间冒烟测试（上传→下载→删除→确认对象被清理），验证一次通过
- [ ] 9.4 编写“安全与隐私”页面（服务端可见信息清单、Web E2EE 局限、丢失密码+恢复密钥的后果），Playwright 验证页面可访问
- [ ] 9.5 全链路验收：在 docker-compose 环境用 Playwright 跑完整用户旅程，并执行日志敏感信息扫描（无令牌/密钥/预签名签名），全部通过
