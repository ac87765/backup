# 端对端加密备份

浏览器内完成加密与解密的个人文件备份服务：文件以密文形式存放在阿里云 OSS，
服务端只保存账号、树结构与配额等元数据，看不到文件名、文件内容或任何密钥。

当前仓库处于 **第 1 组：工程脚手架**。密码学核心、HTTP API、上传/下载管线等按
`openspec/changes/bootstrap-e2ee-backup-mvp/tasks.md` 的分组逐步实现。

## 设计与契约

全部行为契约、设计决策与任务清单都在 OpenSpec change 中，实现前请先读：

- [proposal.md](openspec/changes/bootstrap-e2ee-backup-mvp/proposal.md) —— 范围与不在范围内的内容
- [design.md](openspec/changes/bootstrap-e2ee-backup-mvp/design.md) —— 架构与 D1–D16 决策（约束，不是建议）
- [specs/](openspec/changes/bootstrap-e2ee-backup-mvp/specs) —— 可验证的行为场景
- [tasks.md](openspec/changes/bootstrap-e2ee-backup-mvp/tasks.md) —— 分组任务与验证条件
- [AGENTS.md](AGENTS.md) —— 工程硬性规则（加密、服务端、日志、提交规范）

## 前置要求

| 工具 | 版本 | 说明 |
|---|---|---|
| Rust | 1.98.1 | 由 `rust-toolchain.toml` 固定，rustup 首次构建时自动安装（含 rustfmt、clippy、wasm32 目标） |
| Node.js | 24+ | 前端构建与测试 |
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

停止并删除数据卷：

```bash
docker compose -f deploy/docker-compose.dev.yml down -v
```

### 2. 后端

```bash
cargo build --workspace
cargo run -p server
```

> 第 1 组的 `server` 是空实现（不监听端口）。配置加载、HTTP 服务与迁移在第 4 组接入，
> 届时本文会补充所需环境变量。

### 3. 前端

```bash
cd web
npm ci
npm run dev        # http://localhost:5173
```

### 4. 浏览器加密模块（WASM）

```bash
wasm-pack build crates/crypto-wasm --target web --release --no-opt
```

`--no-opt` 跳过 binaryen（`wasm-opt`）下载，用于本地与 CI 的功能验证；
带 `wasm-opt` 的发布构建在镜像构建阶段执行（第 9 组）。产物在
`crates/crypto-wasm/pkg/`，已被 `.gitignore` 忽略。

## 检查命令

提交前（以及每组完成时）必须全部通过：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check

cd web
npm run typecheck
npm run lint
npm test
npm run build
```

后续涉及 WASM 测试与数据库查询时，还要追加：

```bash
wasm-pack test --headless --chrome crates/crypto-wasm
cargo sqlx prepare --workspace --check
openspec validate bootstrap-e2ee-backup-mvp --strict
```

`cargo deny` 需要单独安装：`cargo install cargo-deny --locked`。

## CI

[.github/workflows/ci.yml](.github/workflows/ci.yml) 包含三个并行任务：

- `Rust`：fmt、clippy（`-D warnings`）、test、cargo deny、WASM 构建
- `Web`：typecheck、lint、test、build
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
