#!/usr/bin/env bash
# 从备份恢复数据库（会覆盖当前数据库！）。
#
#   /opt/backup/restore-db.sh /opt/backup/db-backups/backup-YYYYmmdd-HHMMSS.dump --yes
#
# 步骤：先把当前数据库再备份一份 → 停止 server → 删除并重建数据库 → pg_restore → 启动 server。
# 恢复后，备份时间点之后上传的文件在 OSS 中仍然存在，但数据库里没有记录；
# 服务的孤儿对象扫描任务会按规则把它们清理掉。
set -euo pipefail
dump="${1:?用法：restore-db.sh <dump 文件> --yes}"
[ "${2:-}" = "--yes" ] || { echo "这会覆盖当前数据库。确认后在末尾加 --yes 重新运行。" >&2; exit 1; }
[ -f "$dump" ] || { echo "找不到备份文件：$dump" >&2; exit 1; }
cd /opt/backup

echo "== 恢复前先备份当前数据库"
/opt/backup/db-backup.sh

echo "== 停止 server"
docker compose stop server

echo "== 重建数据库并恢复：$dump"
docker compose exec -T postgres dropdb -U backup --force --if-exists backup
docker compose exec -T postgres createdb -U backup backup
docker compose exec -T postgres pg_restore -U backup -d backup --no-owner --exit-on-error < "$dump"

echo "== 启动 server"
docker compose up -d server
for _ in $(seq 1 45); do
  if curl -fsS http://127.0.0.1:8080/readyz > /tmp/backup-readyz.json 2>/dev/null; then
    echo "readyz: $(cat /tmp/backup-readyz.json)"
    exit 0
  fi
  sleep 2
done
echo "server 未就绪，请查看：docker compose logs --tail 50 server" >&2
exit 1
