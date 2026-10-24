# 端对端加密备份

浏览器内完成加密与解密的个人文件备份服务：文件以密文形式存放在阿里云 OSS，
服务端只保存账号、树结构与配额等元数据，看不到文件名、文件内容或任何密钥。

当前仓库处于 **第 4 组：服务端基础设施**。密码学核心、HTTP API、上传/下载管线等按
`openspec/changes/bootstrap-e2ee-backup-mvp/tasks.md` 的分组逐步实现。

## 设计与契约

全部行为契约、设计决策与任务清单都在 OpenSpec change 中，实现前请先读：

- [proposal.md](openspec/changes/bootstrap-e2ee-backup-mvp/proposal.md) —— 范围与不在范围内的内容
- [design.md](openspec/changes/bootstrap-e2ee-backup-mvp/design.md) —— 架构与 D1–D16 决策（约束，不是建议）
- [specs/](openspec/changes/bootstrap-e2ee-backup-mvp/specs) —— 可验证的行为场景
- [tasks.md](openspec/changes/bootstrap-e2ee-backup-mvp/tasks.md) —— 分组任务与验证条件
- [docs/crypto-format.md](docs/crypto-format.md) —— 密钥层级、密文帧与 AAD 格式（`0x01`，永久不可变）
- [AGENTS.md](AGENTS.md) —— 工程硬性规则（加密、服务端、日志、提交规范）

## 前置要求

| 工具 | 版本 | 说明 |
|---|---|---|
| Rust | 1.98.1 | 由 `rust-toolchain.toml` 固定，rustup 首次构建时自动安装（含 rustfmt、clippy、wasm32 目标） |
| Node.js | 见 `.nvmrc`（当前 26.11.1） | 前端构建与测试；CI 用同一个文件确定版本 |
| Docker + Compose v2 | 任意较新版本 | 本地 PostgreSQL 与 MinIO |
| wasm-pack | 0.15+ | 构建浏览器加密模块，CI 中固定为 0.15.0 |

wasm-pack 安装方式（任选其一）：

```bash
cargo install wasm-pack --locked
# 或使用官方安装脚本：https://rustwasm.github.io/wasm-pack/installer/
```

## 仓库结构

```
crates/crypto-core/     # 纯 Rust 加密与格式，无 IO；测试向量在 testdata/
crates/crypto-wasm/     # wasm-bindgen 绑定，供 Web Worker 加载
crates/server/          # Axum 服务（第 4 组起实现）
web/                    # Vite + React + TypeScript 前端
deploy/                 # docker-compose.dev.yml、MinIO 初始化脚本、后续的 Dockerfile 与 OSS 配置
openspec/               # 行为契约与任务（不要修改 spec/design）
```

## 本地开发

### 1. 启动依赖服务（PostgreSQL 16 + MinIO）

在仓库根目录执行：

```bash
docker compose -f deploy/docker-compose.dev.yml up -d --wait
docker compose -f deploy/docker-compose.dev.yml ps
```

`minio-init` 一次性任务会自动创建私有 bucket 并设置 bucket 级 CORS，
允许前端开发源 `http://localhost:5173` 对预签名 URL 做 `GET`/`PUT`/`HEAD`，
并暴露 `ETag`。

| 服务 | 地址 | 凭据（仅本地开发） |
|---|---|---|
| PostgreSQL 16 | `127.0.0.1:5432`，库 `backup` | `backup` / `backup-dev-password` |
| MinIO S3 API | `http://127.0.0.1:9000`，bucket `backup` | `backup` / `backup-dev-secret` |
| MinIO 控制台 | http://127.0.0.1:9001 | 同上 |

> **端口冲突**：如果本机 5432 已被其他 PostgreSQL 占用，compose 会因
> `address already in use` 启动失败。此时用 `POSTGRES_PORT` 换一个宿主机端口即可，
> 例如 `POSTGRES_PORT=5433 docker compose -f deploy/docker-compose.dev.yml up -d --wait`；
> 后端连接串相应改用该端口。

停止并删除数据卷：

```bash
docker compose -f deploy/docker-compose.dev.yml down -v
```

### 2. 后端

服务端配置全部来自 `BACKUP_` 前缀的环境变量。本地 compose 起来后可以这样启动：

```bash
export BACKUP_DATABASE_URL='postgres://backup:backup-dev-password@127.0.0.1:5432/backup'
export BACKUP_STORAGE_KIND=s3
export BACKUP_STORAGE_BUCKET=backup
export BACKUP_STORAGE_REGION=us-east-1
export BACKUP_STORAGE_INTERNAL_ENDPOINT='http://127.0.0.1:9000'
export BACKUP_STORAGE_PUBLIC_ENDPOINT='http://127.0.0.1:9000'
export BACKUP_STORAGE_ACCESS_KEY_ID=backup
export BACKUP_STORAGE_SECRET_ACCESS_KEY=backup-dev-secret
export BACKUP_STORAGE_ALLOW_HTTP=true

cargo run -p server -- migrate   # 发布流程中先执行迁移
cargo run -p server -- serve     # 启动 HTTP API + 前端静态文件
cargo run -p server -- openapi   # 输出 OpenAPI 规范到 stdout（不需要配置/数据库）
```

`serve` 启动时会校验迁移版本：数据库落后于代码、或已应用迁移的校验和不一致时拒绝启动；
数据库版本领先于代码（回滚场景）允许启动。`/healthz` 是存活检查，`/readyz` 是就绪检查
（数据库、迁移版本、对象存储各带 2 秒独立超时）。Prometheus 指标默认在
`127.0.0.1:9091/metrics` 单独端口提供。

配置项（未列出的都有默认值）：

| 变量 | 默认值 | 说明 |
|---|---|---|
| `BACKUP_BIND_ADDR` | `127.0.0.1:8080` | HTTP 监听地址 |
| `BACKUP_METRICS_ADDR` | `127.0.0.1:9091` | Prometheus 指标监听地址（默认只绑定回环） |
| `BACKUP_STATIC_DIR` | `web/dist` | 前端构建产物目录 |
| `BACKUP_DATABASE_URL` | 必填 | PostgreSQL 连接串（密钥） |
| `BACKUP_DATABASE_MAX_CONNECTIONS` | `10` | 连接池上限 |
| `BACKUP_STORAGE_KIND` | 必填 | `s3`（MinIO/测试）或 `oss`（阿里云） |
| `BACKUP_STORAGE_BUCKET` | 必填 | bucket 名称 |
| `BACKUP_STORAGE_REGION` | `us-east-1`（`oss` 必填） | 对象存储 region |
| `BACKUP_STORAGE_INTERNAL_ENDPOINT` | 必填 | 服务端 `stat`/`list`/`delete` 使用的内网 endpoint |
| `BACKUP_STORAGE_PUBLIC_ENDPOINT` | 必填 | 只用于生成预签名 URL 的公网 endpoint，其 origin 会追加到 CSP `connect-src` |
| `BACKUP_STORAGE_ACCESS_KEY_ID` | 必填 | 访问密钥 ID（密钥） |
| `BACKUP_STORAGE_SECRET_ACCESS_KEY` | 必填 | 访问密钥（密钥） |
| `BACKUP_STORAGE_ALLOW_HTTP` | `false` | 允许 http endpoint（本地 MinIO 需要设为 `true`） |
| `BACKUP_JOBS_ENABLED` | `true` | 是否在本实例运行后台任务（关闭便于拆分独立 worker） |
| `BACKUP_MAIL_MODE` | `file` | `file`（写本地目录，不真正发送）或 `smtp` |
| `BACKUP_MAIL_DIR` | `var/mail` | `file` 模式的邮件目录 |
| `BACKUP_SMTP_HOST` | `smtp` 模式必填 | SMTP 主机 |
| `BACKUP_SMTP_PORT` | `587` | SMTP 端口 |
| `BACKUP_SMTP_FROM` | `smtp` 模式必填 | 发件人地址 |
| `BACKUP_SMTP_USERNAME` | 可选 | SMTP 用户名（与密码必须同时配置） |
| `BACKUP_SMTP_PASSWORD` | 可选 | SMTP 密码（密钥） |
| `BACKUP_SMTP_TLS` | `starttls` | `starttls` 或 `none`（本地假 SMTP 测试用） |

密钥类配置在日志与 `Debug` 输出中固定显示为 `[REDACTED]`。缺项或非法值时进程以非零退出码
退出，日志只指出配置项名称，不输出配置值。`BACKUP_MAIL_MODE=file` 启动时会输出一条
WARN，提示邮件未真正发送。

服务端集成测试使用 testcontainers 启动真实 PostgreSQL 与 MinIO，因此运行
`cargo test --workspace` 需要可用的 Docker（Windows 本机没有原生 Docker，请在有 Docker 的
WSL 里执行；测试不会在缺少 Docker 时静默跳过，而是明确失败）。

### 3. 前端

```bash
cd web
npm ci
npm run dev        # http://localhost:5173
```

`npm run dev` / `npm run build` / `npm test` 会先执行 `npm run build:wasm`，把
`crypto-wasm` 打包到 `web/src/generated/crypto-wasm/`（已被 `.gitignore` 忽略），
所以本地需要先装好 wasm-pack。

API 类型由服务端 OpenAPI 生成（生成物 `src/api/schema.d.ts` 已提交，不要手改）：

```bash
npm run generate:api   # 需要 Rust；等价于 cargo run -p server -- openapi + openapi-typescript
```

`web/.npmrc` 设置 `legacy-peer-deps=true`：`openapi-typescript` 的 peerDependency
仍声明 TypeScript 5，而本仓库使用 TypeScript 6；该工具只做规范到类型的转换，
不调用 tsc API。

> **WASM 只在 Web Worker 中加载**：主线程通过 `src/worker/client.ts` 的 Comlink 代理
> 调用加密操作，永远不接触密钥。ESLint（`no-restricted-imports`）与
> `src/worker/isolation.test.ts` 会强制这一点。

### 4. 浏览器加密模块（WASM）

```bash
wasm-pack build crates/crypto-wasm --target web --release --no-opt
```

`--no-opt` 跳过 binaryen（`wasm-opt`）下载，用于本地与 CI 的功能验证；
带 `wasm-opt` 的发布构建在镜像构建阶段执行（第 9 组）。产物在
`crates/crypto-wasm/pkg/`，已被 `.gitignore` 忽略。

浏览器内测试（需要本机 Chrome；ChromeDriver 与 Chrome 大版本必须一致，
必要时用 `--chromedriver <path>` 指定）：

```bash
wasm-pack test --headless --chrome --release crates/crypto-wasm
```

### 5. 端到端测试（CSP）

```bash
cd web
npx playwright install chromium   # 首次
npm run test:e2e
```

Playwright 会以 `VITE_SELFTEST=1` 构建生产产物，并用 `vite preview` 加
`script-src 'self' 'wasm-unsafe-eval'` 的 CSP 响应头，然后验证一次完整的
Worker 加解密往返与锁定错误码。

## 检查命令

提交前（以及每组完成时）必须全部通过：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
SQLX_OFFLINE=true cargo test --workspace   # server 集成测试会用 testcontainers 启动 PostgreSQL/MinIO
cargo sqlx prepare --workspace --check      # 校验 .sqlx/ 与查询宏一致（CI 也执行）
cargo deny check

cd web
npm run typecheck
npm run lint
npm test
npm run build
npm run generate:api && git diff --exit-code -- src/api/schema.d.ts  # OpenAPI 生成物无漂移
```

涉及 WASM 与端到端时，还要追加：

```bash
wasm-pack test --headless --chrome --release crates/crypto-wasm
cd web && npm run test:e2e
openspec validate bootstrap-e2ee-backup-mvp --strict
```

`cargo deny` 需要单独安装：`cargo install cargo-deny --locked`。

## CI

[.github/workflows/ci.yml](.github/workflows/ci.yml) 包含五个并行任务：

- `Rust`：fmt、clippy（`-D warnings`）、`cargo sqlx prepare --check`（离线查询缓存）、
  test（testcontainers 启动真实 PostgreSQL/MinIO）、符号泄漏检查、cargo deny、
  `wasm-pack test --headless --chrome`；并在此 job 只构建一次 WASM 绑定和
  `openapi.json`，用 artifact 传给后续 job
- `Web`：下载 WASM 与 OpenAPI artifact（不安装 Rust/wasm-pack），typecheck、lint、
  test、build，并用 artifact 规范重新生成 OpenAPI 类型后检查无漂移
- `E2E`：下载 WASM artifact，Playwright 在生产构建 + 严格 CSP 下跑 WASM 冒烟用例
- `OpenSpec`：固定 CLI 版本执行 `validate --strict`
- `Dev stack`：实际启动 `deploy/docker-compose.dev.yml`，等待 PostgreSQL/MinIO 健康，
  并执行 `minio-init` 验证 bucket 与 CORS 配置

## 环境变量（dev compose 可覆盖）

dev 服务全部有默认值，通常不需要设置；需要覆盖时在仓库根目录建 `.env`（已被忽略）：

| 变量 | 默认值 | 说明 |
|---|---|---|
| `POSTGRES_VERSION` | `16.15-alpine` | PostgreSQL 镜像 tag |
| `POSTGRES_DB` / `POSTGRES_USER` / `POSTGRES_PASSWORD` | `backup` / `backup` / `backup-dev-password` | 本地数据库 |
| `POSTGRES_PORT` | `5432` | 宿主机端口 |
| `MINIO_IMAGE` / `MINIO_CLIENT_IMAGE` | Bitnami 归档镜像 | 可指向内部镜像仓库 |
| `MINIO_ROOT_USER` / `MINIO_ROOT_PASSWORD` | `backup` / `backup-dev-secret` | MinIO 根凭据 |
| `MINIO_BUCKET` | `backup` | 开发用 bucket |
| `CORS_ALLOWED_ORIGIN` | `http://localhost:5173` | 允许直传对象存储的前端源 |

## 已知限制

- **MinIO 官方镜像已从公共仓库（Docker Hub、quay.io）下架**，因此本地 dev 环境使用
  Bitnami 的归档镜像 `bitnamilegacy/minio` / `bitnamilegacy/minio-client`。
  它们只用于本地开发，不承载真实数据；生产使用阿里云 OSS。可用 `MINIO_IMAGE`、
  `MINIO_CLIENT_IMAGE` 切换到内部镜像仓库。
- Web 版 E2EE 依赖服务端下发的 JS 未被篡改（这是 Web 平台的固有限制），
  隐私说明页面在第 9 组实现。
- 没有 Docker 的环境无法运行 dev compose；CI 的 `Dev stack` 任务会做实际验证。

## 提交规范

Conventional Commits：`feat(server): ...`、`feat(crypto): ...`、`feat(web): ...`、
`test(...)`、`chore(...)`。每个提交都要可编译、测试通过。
