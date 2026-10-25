# 单镜像多阶段构建（design D15 / task 9.1）：
#   planner/builder  cargo-chef 缓存依赖 → release 构建 server
#   wasm-builder    wasm-pack 0.15.0 构建 crypto-wasm
#   web-builder     Node（.nvmrc）+ Vite 构建前端
#   runtime         debian:bookworm-slim，非 root，仅 server 二进制 + 前端 + CA 证书
#
# 同时兼容 Docker BuildKit 与传统 builder：不使用 RUN --mount、COPY --link、
# heredoc 等仅 BuildKit 支持的语法；TARGETARCH 在传统 builder 下为空时，
# wasm-builder 用 `dpkg --print-architecture` 回退识别实际架构。
#
# 迁移文件由 `sqlx::migrate!` 编译进 server 二进制；运行时使用 `server migrate`
# 与 `server serve` 两个子命令（design D15）。
#
# 所有基础镜像按 manifest list 的 digest 固定；注释保留可读 tag。
# Rust 版本由 rust-toolchain.toml 固定为 1.98.1，Node 版本由 .nvmrc 固定为 26.11.1，
# wasm-pack 与 CI 同为 0.15.0。升级时必须同时更新这些文件与注释。

# ---------------------------------------------------------------------------
# cargo-chef：依赖缓存层
# ---------------------------------------------------------------------------
FROM rust:1.98.1-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS chef

# cargo-chef 只用于生成/复用依赖 recipe；版本固定，升级时与 Dockerfile 一起改。
RUN cargo install cargo-chef --version 0.1.78 --locked

WORKDIR /app

# ---------------------------------------------------------------------------
# planner：把依赖清单与源码结构做成 recipe（源码变化不会使依赖层失效）
# ---------------------------------------------------------------------------
FROM chef AS planner

COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ---------------------------------------------------------------------------
# builder：先只编译依赖，再编译 server release 二进制
# ---------------------------------------------------------------------------
FROM chef AS builder

# 编译期 SQL 校验读取仓库中提交的 .sqlx 缓存；镜像构建不连接数据库。
ENV SQLX_OFFLINE=true
ENV CARGO_INCREMENTAL=0

COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

COPY . .
RUN cargo build --release --locked -p server

# ---------------------------------------------------------------------------
# wasm-builder：构建浏览器加密 WASM
# ---------------------------------------------------------------------------
FROM rust:1.98.1-bookworm@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e AS wasm-builder

# CI 与本地都使用 x86_64 Linux；这里使用与 CI 完全相同的官方 wasm-pack 发布包。
# 若需要在其他架构构建，请先补充对应架构的发布包（不要静默换版本）。
#
# BuildKit 会自动注入 TARGETARCH；传统 builder 不会，此时用 dpkg 查询实际架构。
# `dpkg --print-architecture` 在 Debian 基础镜像中始终可用，避免依赖 uname 映射。
ARG TARGETARCH
RUN set -eu; \
    arch="${TARGETARCH:-}"; \
    if [ -z "$arch" ]; then \
      arch="$(dpkg --print-architecture)"; \
    fi; \
    echo "building wasm for architecture: $arch"; \
    case "$arch" in \
      amd64) ;; \
      *) \
        echo "wasm-pack 0.15.0 当前仅配置了 linux/amd64 发布包（实际架构：$arch）" >&2; \
        exit 1; \
        ;; \
    esac

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL -o /tmp/wasm-pack.tar.gz \
      https://github.com/wasm-bindgen/wasm-pack/releases/download/v0.15.0/wasm-pack-v0.15.0-x86_64-unknown-linux-musl.tar.gz \
    && tar -xzf /tmp/wasm-pack.tar.gz -C /tmp \
    && install /tmp/wasm-pack-v0.15.0-x86_64-unknown-linux-musl/wasm-pack /usr/local/bin/wasm-pack \
    && rm -rf /tmp/wasm-pack.tar.gz /tmp/wasm-pack-v0.15.0-x86_64-unknown-linux-musl \
    && wasm-pack --version

WORKDIR /app
COPY . .

# --no-opt 与 CI 一致：不依赖 binaryen 下载；发布质量优先保证可复现与可验证。
RUN wasm-pack build crates/crypto-wasm \
      --target web --release --no-opt \
      --out-dir ../../web/src/generated/crypto-wasm \
      --out-name crypto_wasm

# ---------------------------------------------------------------------------
# web-builder：Vite 生产构建
# ---------------------------------------------------------------------------
FROM node:26.11.1-bookworm-slim@sha256:86f07bc9c5dce4578cf37e5a418b7bfc7f817cda25cde66e2b66e95ed86c4567 AS web-builder

WORKDIR /app

COPY web ./web
# vite.config.ts 读取仓库根部的共享 CSP 策略文件。
COPY csp ./csp
# 使用 wasm-builder 的产物；SKIP_WASM_BUILD=1 只检查产物存在，不再重复构建。
COPY --from=wasm-builder /app/web/src/generated/crypto-wasm ./web/src/generated/crypto-wasm

WORKDIR /app/web
ENV SKIP_WASM_BUILD=1
RUN npm ci
RUN npm run build

# ---------------------------------------------------------------------------
# runtime：最小运行镜像
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim@sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8 AS runtime

# 只安装必要的 CA 证书（HTTPS 出站：RDS、OSS、SMTP）。运行时不需要 curl/tzdata。
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# server 二进制与前端静态文件；迁移已嵌入二进制。
COPY --from=builder --chown=10001:10001 /app/target/release/server /usr/local/bin/server
COPY --from=web-builder --chown=10001:10001 /app/web/dist /app/web/dist

# 数字 UID 非 root 运行（无需 passwd/adduser 包）。预创建邮件目录的属主，
# 使 docker named volume 初始化后该用户仍可写（仅 file 模式/端到端测试使用）。
RUN mkdir -p /var/lib/backup/mail \
    && chown -R 10001:10001 /app /var/lib/backup/mail

ENV BACKUP_STATIC_DIR=/app/web/dist \
    BACKUP_BIND_ADDR=0.0.0.0:8080 \
    BACKUP_METRICS_ADDR=0.0.0.0:9091 \
    BACKUP_MAIL_DIR=/var/lib/backup/mail

USER 10001:10001

EXPOSE 8080 9091

# 服务端处理 SIGTERM 时优雅停机（service-operations spec）。
STOPSIGNAL SIGTERM

ENTRYPOINT ["/usr/local/bin/server"]
CMD ["serve"]
