#!/usr/bin/env bash
# 回滚到上一个镜像（或指定镜像）。不回退数据库：迁移向前兼容，旧镜像可在新 schema 上运行。
#
#   /opt/backup/rollback.sh                       # 回到 upgrade.sh 记录的上一个版本
#   /opt/backup/rollback.sh backup-server:a0bdb77  # 回到指定版本（docker images backup-server 查看）
set -euo pipefail
cd /opt/backup
target="${1:-$(cat .previous-image 2>/dev/null || true)}"
[ -n "$target" ] || { echo "没有记录上一个版本；请指定镜像，例如 rollback.sh backup-server:a0bdb77" >&2; exit 1; }
docker image inspect "$target" >/dev/null || { echo "本机没有镜像：$target" >&2; exit 1; }
current=$(sed -n 's/^BACKUP_IMAGE=//p' .env)
echo "== 回滚：$current → $target"
sed -i "s|^BACKUP_IMAGE=.*|BACKUP_IMAGE=$target|" .env
echo "$current" > .previous-image
docker compose up -d server
for _ in $(seq 1 45); do
  if curl -fsS http://127.0.0.1:8080/readyz > /tmp/backup-readyz.json 2>/dev/null; then
    echo "readyz: $(cat /tmp/backup-readyz.json)"
    exit 0
  fi
  sleep 2
done
echo "回滚后未就绪，请查看：docker compose logs --tail 50 server" >&2
exit 1
