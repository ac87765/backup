#!/usr/bin/env bash
# 升级到新镜像（在服务器上运行；release.sh 会自动调用）。
#
#   /opt/backup/upgrade.sh /root/backup-release.tar.gz   # docker save 导出的镜像包（gzip）
#   /opt/backup/upgrade.sh backup-server:abc1234         # 已加载到本机的镜像
#
# 步骤：加载镜像 → 备份数据库 → 记录当前镜像（供回滚）→ 切换并启动（启动时自动执行迁移）
# → 等待 /readyz。新版本 90 秒内没有就绪时自动回滚到之前的镜像并以非零状态退出。
# 迁移只做向前兼容的增量变更，旧镜像可以在新 schema 上运行，所以回滚不需要回退数据库。
set -euo pipefail
target="${1:?用法：upgrade.sh <镜像包.tar.gz | 镜像名:标签>}"
cd /opt/backup

# 客户端下载目录（D4）：升级时幂等创建；Caddy 以 caddy 用户运行，父目录需可穿越。
install -d -o root -g root -m 755 /opt/backup/downloads
chmod 711 /opt/backup

if [ -f "$target" ]; then
  echo "== 加载镜像包：$target"
  loaded=$(gunzip -c "$target" | docker load | sed -n 's/^Loaded image: //p' | tail -n1)
  [ -n "$loaded" ] || { echo "镜像包里没有找到镜像" >&2; exit 1; }
  image="$loaded"
  rm -f "$target"
else
  image="$target"
  docker image inspect "$image" >/dev/null || { echo "本机没有镜像：$image" >&2; exit 1; }
fi

current=$(sed -n 's/^BACKUP_IMAGE=//p' .env)
echo "== 当前：$current → 目标：$image"
if [ "$current" = "$image" ]; then
  echo "已经是该版本，只确保服务在运行"
fi

wait_ready() {
  for _ in $(seq 1 45); do
    if curl -fsS http://127.0.0.1:8080/readyz > /tmp/backup-readyz.json 2>/dev/null; then
      echo "readyz: $(cat /tmp/backup-readyz.json)"
      return 0
    fi
    sleep 2
  done
  return 1
}

switch_to() {
  sed -i "s|^BACKUP_IMAGE=.*|BACKUP_IMAGE=$1|" .env
  docker compose up -d server
}

echo "== 备份数据库"
/opt/backup/db-backup.sh

if [ "$current" != "$image" ]; then
  echo "$current" > .previous-image
fi
switch_to "$image"
if wait_ready; then
  echo "== 升级完成：$image"
  exit 0
fi

echo "== 新版本未就绪，最近日志：" >&2
docker compose logs --no-color --tail 40 server >&2 || true
if [ "$current" != "$image" ] && [ "$current" != "backup-server:not-deployed" ]; then
  echo "== 自动回滚到 $current" >&2
  switch_to "$current"
  wait_ready || echo "回滚后仍未就绪，请人工检查" >&2
fi
exit 1
