#!/bin/sh
# 为本地开发准备 MinIO：创建私有 bucket，并配置 CORS。
# 由 docker-compose.dev.yml 的 minio-init 服务执行，可重复运行（幂等）。
#
# 依赖：镜像内提供 mc（MinIO Client）。
set -eu

: "${MINIO_ENDPOINT:=http://minio:9000}"
: "${MINIO_BUCKET:=backup}"
: "${CORS_ALLOWED_ORIGIN:=http://localhost:5173}"

ALIAS="backup-dev"
CORS_FILE="/tmp/cors.xml"

log() {
	printf '[minio-init] %s\n' "$*"
}

log "等待 MinIO 就绪：${MINIO_ENDPOINT}"
attempt=0
while ! mc alias set "$ALIAS" "$MINIO_ENDPOINT" "$MINIO_ROOT_USER" "$MINIO_ROOT_PASSWORD" >/dev/null 2>&1; do
	attempt=$((attempt + 1))
	if [ "$attempt" -ge 30 ]; then
		log "MinIO 在 60 秒内未就绪，放弃"
		exit 1
	fi
	sleep 2
done

log "创建 bucket（若不存在）：${MINIO_BUCKET}"
mc mb --ignore-existing "$ALIAS/$MINIO_BUCKET" >/dev/null

log "将 bucket 设为私有：浏览器只能通过预签名 URL 访问"
mc anonymous set none "$ALIAS/$MINIO_BUCKET" >/dev/null

# CORS：
# - 社区版 MinIO 不支持 S3 的 PutBucketCors（会返回 NotImplemented），其 CORS 由
#   服务端环境变量 MINIO_API_CORS_ALLOW_ORIGIN 统一设置（见 docker-compose.dev.yml）。
# - 对支持 bucket 级 CORS 的实现（例如真实 OSS），这里再显式写入同样的规则。
# 只允许前端源做 GET/PUT/HEAD，并暴露 ETag 供上传确认使用。
cat >"$CORS_FILE" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<CORSConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <CORSRule>
    <AllowedOrigin>${CORS_ALLOWED_ORIGIN}</AllowedOrigin>
    <AllowedMethod>GET</AllowedMethod>
    <AllowedMethod>PUT</AllowedMethod>
    <AllowedMethod>HEAD</AllowedMethod>
    <AllowedHeader>*</AllowedHeader>
    <ExposeHeader>ETag</ExposeHeader>
    <MaxAgeSeconds>3000</MaxAgeSeconds>
  </CORSRule>
</CORSConfiguration>
EOF

if mc cors set "$ALIAS/$MINIO_BUCKET" "$CORS_FILE" >/dev/null 2>&1; then
	log "bucket 级 CORS 已设置（允许来源：${CORS_ALLOWED_ORIGIN}）"
else
	log "对象存储不支持 bucket 级 CORS，沿用服务端 CORS（本地 MinIO 由 MINIO_API_CORS_ALLOW_ORIGIN 提供）"
fi

# 刻意不开启 bucket 版本控制：版本由应用管理（design D15），
# 开启对象存储版本控制会导致删除不释放空间。

log "bucket 与 CORS 配置完成"
