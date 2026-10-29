#!/usr/bin/env bash
# 一键发布（在开发机上运行，需要 git、docker、ssh；Windows 上在 WSL 里运行）。
#
#   SERVER=root@120.79.245.99 SSH_KEY=~/.ssh/id_ed25519 deploy/server/release.sh
#
# 可选变量：
#   REV=<commit>          要发布的提交（默认 HEAD；只打包已提交的内容，工作区改动不会进镜像）
#   BUILD_PROXY=<url>     构建容器访问 GitHub/crates.io 用的 HTTP 代理（国内网络常需要），
#                         例如 http://127.0.0.1:10808；会同时使用 --network host
#
# 步骤：git archive 导出 REV → docker build（镜像名 backup-server:<短 SHA>）→ docker save | gzip
# → 上传镜像包，并同步 compose.yml 与运维脚本到 /opt/backup → 服务器上执行 upgrade.sh
#（先备份数据库，失败自动回滚）。服务器上的 .env / backup.env 不会被改动。
# 服务器无法访问 Docker Hub 也没关系：镜像在本机构建后整体上传。
set -euo pipefail

SERVER="${SERVER:?需要设置 SERVER，例如 root@120.79.245.99}"
REV="${REV:-HEAD}"
repo="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
sha="$(git -C "$repo" rev-parse --short=7 "$REV")"
image="backup-server:${sha}"
ssh_opts=(-o BatchMode=yes)
[ -n "${SSH_KEY:-}" ] && ssh_opts+=(-i "$SSH_KEY")

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "== 导出 $REV（$sha）"
mkdir -p "$work/src"
git -C "$repo" archive "$REV" | tar -x -C "$work/src"

echo "== 构建镜像 $image（release 构建，首次约 15–30 分钟，之后走缓存）"
build_args=()
if [ -n "${BUILD_PROXY:-}" ]; then
  build_args+=(--network host --build-arg "HTTPS_PROXY=$BUILD_PROXY" --build-arg "HTTP_PROXY=$BUILD_PROXY")
fi
docker build "${build_args[@]}" -t "$image" "$work/src"

echo "== 导出镜像包"
docker save "$image" | gzip -1 > "$work/release.tar.gz"
ls -lh "$work/release.tar.gz" | awk '{print "镜像包大小：" $5}'

echo "== 上传镜像包与运维脚本到 $SERVER"
scp "${ssh_opts[@]}" -q "$work/release.tar.gz" "$SERVER:/root/backup-release.tar.gz"
scp "${ssh_opts[@]}" -q \
  "$repo/deploy/server/compose.yml" \
  "$repo/deploy/server/db-backup.sh" \
  "$repo/deploy/server/restore-db.sh" \
  "$repo/deploy/server/upgrade.sh" \
  "$repo/deploy/server/rollback.sh" \
  "$repo/deploy/server/get-link.sh" \
  "$SERVER:/opt/backup/"
ssh "${ssh_opts[@]}" "$SERVER" 'chmod 755 /opt/backup/*.sh && chmod 644 /opt/backup/compose.yml'

echo "== 服务器上升级"
ssh "${ssh_opts[@]}" "$SERVER" "/opt/backup/upgrade.sh /root/backup-release.tar.gz"
echo "== 发布完成：$image"
