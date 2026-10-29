#!/usr/bin/env bash
# 数据库备份：pg_dump 自定义格式（已压缩），保存到 /opt/backup/db-backups，保留 KEEP_DAYS 天（默认 14）。
# 由 /etc/cron.d/backup-db 每天 03:30 运行；upgrade.sh / restore-db.sh 也会先调用它。
#
# 注意：这里备份的是“账号与文件结构”元数据（文件名本身是密文）。文件内容在 OSS 中，
# 不在这个备份里；OSS 的数据可靠性由 OSS 自身保证（可按需开启跨区域复制）。
set -euo pipefail
umask 077
cd /opt/backup
KEEP_DAYS="${KEEP_DAYS:-14}"
out="/opt/backup/db-backups/backup-$(date +%Y%m%d-%H%M%S).dump"
docker compose exec -T postgres pg_dump -U backup -d backup -Fc > "$out.tmp"
mv "$out.tmp" "$out"
find /opt/backup/db-backups -name 'backup-*.dump' -mtime +"$KEEP_DAYS" -delete
echo "$(date -Is) dump ok: $out ($(du -h "$out" | cut -f1))"
