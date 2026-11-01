#!/usr/bin/env bash
# 首次安装（在服务器上以 root 运行；可重复执行，已有的密钥与配置不会被覆盖）。
#
#   scp -r deploy/server root@<host>:/root/backup-deploy
#   ssh root@<host> 'SITE_HOST=<域名或公网IP> bash /root/backup-deploy/install.sh'
#
# 做的事：swap（内存 < 2 GiB 时）→ Docker 与 Compose → Caddy → /opt/backup 目录与脚本
# → 生成 .env 与 backup.env（含随机密钥）→ 渲染 Caddyfile → 每日数据库备份定时任务。
# 不会启动应用：填写 backup.env 里的 OSS/SMTP 后，用 release.sh（或 upgrade.sh）部署镜像。
set -euo pipefail

SITE_HOST="${SITE_HOST:?需要设置 SITE_HOST（域名或公网 IP）}"
DEST=/opt/backup
SRC="$(cd "$(dirname "$0")" && pwd)"

[ "$(id -u)" = 0 ] || { echo "请以 root 运行" >&2; exit 1; }
command -v apt-get >/dev/null || { echo "只支持 Debian/Ubuntu" >&2; exit 1; }
export DEBIAN_FRONTEND=noninteractive

step() { printf '\n== %s\n' "$1"; }

step "swap"
mem_kib=$(awk '/MemTotal/ {print $2}' /proc/meminfo)
if [ "$mem_kib" -lt 2097152 ] && ! swapon --show | grep -q .; then
  fallocate -l 2G /swapfile && chmod 600 /swapfile && mkswap /swapfile >/dev/null && swapon /swapfile
  grep -q '^/swapfile ' /etc/fstab || echo '/swapfile none swap sw 0 0' >> /etc/fstab
  echo 'vm.swappiness=20' > /etc/sysctl.d/90-backup-swap.conf && sysctl -q -p /etc/sysctl.d/90-backup-swap.conf
  echo "已创建 2 GiB swap"
else
  echo "跳过（内存充足或已有 swap）"
fi

step "Docker 与 Compose"
apt-get update -qq
apt-get install -y -qq ca-certificates curl gnupg openssl docker.io docker-compose-v2 >/dev/null
systemctl enable --now docker >/dev/null 2>&1
docker --version && docker compose version

step "Caddy（官方仓库；IP 证书需要 2.10 以上）"
if ! command -v caddy >/dev/null; then
  curl -fsSL 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' \
    | gpg --dearmor --yes -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
  curl -fsSL 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' \
    -o /etc/apt/sources.list.d/caddy-stable.list
  apt-get update -qq && apt-get install -y -qq caddy >/dev/null
fi
caddy version

step "目录与脚本：$DEST"
install -d -m 700 "$DEST" "$DEST/db-backups"
# 客户端下载目录：root 拥有、755，由发布脚本写入，Caddy 直接读取（D4）。
# Caddy 在 Debian 上以 caddy 用户运行，需要穿过 /opt/backup 才能读到 downloads；
# 父目录设为 711（可穿越、不可列出），其余文件仍是 600/644。
install -d -o root -g root -m 755 "$DEST/downloads"
chmod 711 "$DEST"
install -m 644 "$SRC/compose.yml" "$DEST/compose.yml"
for script in db-backup.sh restore-db.sh upgrade.sh rollback.sh get-link.sh; do
  install -m 755 "$SRC/$script" "$DEST/$script"
done

step "配置文件（已存在的不会覆盖）"
umask 077
if [ ! -f "$DEST/.env" ]; then
  printf 'POSTGRES_PASSWORD=%s\nBACKUP_IMAGE=backup-server:not-deployed\n' "$(openssl rand -hex 24)" > "$DEST/.env"
  echo "已生成 .env（数据库密码随机生成）"
fi
if [ ! -f "$DEST/backup.env" ]; then
  sed -e "s|https://SITE_HOST|https://${SITE_HOST}|" \
      -e "s|^BACKUP_AUTH_PEPPER=.*|BACKUP_AUTH_PEPPER=$(openssl rand -base64 48 | tr -d '\n')|" \
      -e "s|^BACKUP_AUTH_FAKE_SALT_KEY=.*|BACKUP_AUTH_FAKE_SALT_KEY=$(openssl rand -base64 48 | tr -d '\n')|" \
      "$SRC/backup.env.example" > "$DEST/backup.env"
  echo "已生成 backup.env（pepper 与 fake-salt 密钥随机生成）"
fi
chmod 600 "$DEST/.env" "$DEST/backup.env"

step "Caddyfile"
if [[ "$SITE_HOST" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  global_extra="default_sni ${SITE_HOST}"
  tls_block=$'tls {\n\t\tissuer acme {\n\t\t\tprofile shortlived\n\t\t}\n\t}'
  echo "IP 模式：Let's Encrypt 短期 IP 证书（中国大陆服务器对外提供网站服务需按规定备案）"
else
  global_extra=""
  tls_block=""
  echo "域名模式：Caddy 自动申请证书（请先把域名解析到本机，并放行 80/443）"
fi
rendered=$(mktemp)
SITE_HOST="$SITE_HOST" GLOBAL_EXTRA="$global_extra" TLS_BLOCK="$tls_block" awk '
  { gsub(/\{\{SITE_HOST\}\}/, ENVIRON["SITE_HOST"])
    gsub(/\{\{GLOBAL_EXTRA\}\}/, ENVIRON["GLOBAL_EXTRA"])
    gsub(/\{\{TLS_BLOCK\}\}/, ENVIRON["TLS_BLOCK"])
    print }' "$SRC/Caddyfile.template" > "$rendered"
caddy validate --config "$rendered" --adapter caddyfile >/dev/null
if ! cmp -s "$rendered" /etc/caddy/Caddyfile; then
  [ -f /etc/caddy/Caddyfile ] && cp /etc/caddy/Caddyfile "/etc/caddy/Caddyfile.bak.$(date +%Y%m%d%H%M%S)"
  install -m 644 "$rendered" /etc/caddy/Caddyfile
  systemctl reload caddy || systemctl restart caddy
  echo "Caddyfile 已更新并重新加载"
else
  echo "Caddyfile 无变化"
fi
rm -f "$rendered"

step "每日数据库备份（03:30，保留 14 天）"
echo "30 3 * * * root $DEST/db-backup.sh >> /var/log/backup-db.log 2>&1" > /etc/cron.d/backup-db
chmod 644 /etc/cron.d/backup-db

cat <<EOF

安装完成。接下来：
  1. 编辑 $DEST/backup.env，填写 OSS（bucket、地域、AccessKey）与可选的 SMTP；
  2. 在开发机上运行 deploy/server/release.sh 构建并部署镜像（见 deploy/server/README.md）；
  3. 部署后授予管理员：cd $DEST && docker compose exec -T server /usr/local/bin/server admin grant <你的邮箱>
EOF
