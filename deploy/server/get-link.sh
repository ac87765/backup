#!/usr/bin/env bash
# 取某个邮箱最近一封验证/重置邮件里的链接（仅 BACKUP_MAIL_MODE=file 时有用：邮件写入文件而不发送）。
#
#   /opt/backup/get-link.sh friend@example.com
set -euo pipefail
email="${1:?用法：get-link.sh 邮箱}"
cd /opt/backup
docker compose exec -T server sh -c 'ls -t /var/lib/backup/mail/*.eml 2>/dev/null || true' | while read -r f; do
  raw=$(docker compose exec -T server cat "$f")
  printf '%s' "$raw" | grep -qi -- "$email" || continue
  body=$(printf '%s' "$raw" | awk 'BEGIN{b=0} /^\r?$/{b=1; next} b{print}')
  if printf '%s' "$raw" | grep -qi 'Content-Transfer-Encoding: *base64'; then
    body=$(printf '%s' "$body" | tr -d '\r\n ' | base64 -d 2>/dev/null || true)
  elif printf '%s' "$raw" | grep -qi 'Content-Transfer-Encoding: *quoted-printable'; then
    body=$(printf '%s' "$body" | perl -pe 's/=\r?\n//g; s/=([0-9A-Fa-f]{2})/chr(hex($1))/ge')
  fi
  link=$(printf '%s' "$body" | grep -oE 'https?://[^ "<>]+#token=[0-9a-f]+' | head -n1 || true)
  if [ -n "$link" ]; then echo "$link"; exit 0; fi
done
echo "没有找到发给 $email 的邮件（注册后等几秒再试）" >&2
exit 1
