# 文件夹备份上线 Runbook

本文只覆盖 `add-folder-backup` 带来的部署变化。OSS / RAM / CORS / SSE 的完整配置见
`deploy/oss/README.md`；本文不重复，也不改动该目录下用户正在维护的未提交文件。

## 1. 构建与发布

1. 在合并后的 `main` 上构建镜像（Dockerfile 会重新编译 WASM 与前端）：
   ```bash
   docker build -t backup:folder-backup .
   ```
2. 先运行数据库迁移（本次变更无迁移）：
   ```bash
   docker run --rm --env-file /opt/backup/backup.env backup:folder-backup migrate
   ```
3. 替换 `server` 容器；发布前确认 `/readyz` 返回就绪。
4. 回滚：切回上一镜像标签即可。本次只新增可选版本元数据字段 `content_sha256`，
   旧镜像仍可读取新密文（旧客户端/旧服务端忽略不认识的元数据字段）。

## 2. 提高每用户写限流

文件夹备份中一个小文件约产生 4 次写请求（创建会话、presign、confirm、commit）。
1000 个小文件约 4000 次请求，服务端默认 240/分钟会持续触发 `429 rate_limited`。
客户端会按 `Retry-After` 自动等待并重试（最多 8 次，加 0–20% 抖动），不会把文件
标记为失败，但整体耗时会明显变长。

在 `/opt/backup/backup.env` 中设置：

```dotenv
BACKUP_USER_WRITE_RATE_LIMIT_PER_MINUTE=1200
```

注意：

- 服务端代码默认值仍是 240；这里只改生产环境变量。
- 多实例部署时每个实例各自统计，实际总额约为 `1200 × 实例数`。
- 若前端仍出现持续 429，先检查反向代理/WAF 是否另有更低的限流。
- 客户端日志/UI 中只会出现“服务器繁忙，稍后自动继续”，不包含令牌或密钥。

## 3. 验证

部署后按顺序验证（E2E 需要真实 PostgreSQL 与 S3/MinIO）：

```bash
cd web
npm run typecheck && npm run lint && npm test && npm run build
E2E_SKIP_BUILD=1 npx playwright test e2e/folder.spec.ts
```

- 1000 小文件场景需要同时设置：
  ```bash
  E2E_FOLDER_RATE_LIMIT_TEST=1 E2E_USER_WRITE_RATE_LIMIT=600 \
    E2E_SKIP_BUILD=1 npx playwright test e2e/folder.spec.ts -g 1000
  ```
  `E2E_USER_WRITE_RATE_LIMIT` 只由 E2E 启动脚本传给服务端，不影响默认配置。
- 截图脚本新增文件夹任务面板：
  ```bash
  E2E_SCREENSHOTS=1 E2E_SKIP_BUILD=1 npx playwright test e2e/screenshots.spec.ts
  ```
  输出在 `web/e2e-tmp/screenshots/folder-job-desktop.png`。

## 4. 已知限制

- 空文件夹只能通过拖拽方式保留；`<input webkitdirectory>` 不会提供空目录。
- 定时/后台备份、删除同步不在本次范围（桌面客户端后续实现）。
- 打包下载不压缩（STORE），优先 File System Access；否则走 Service Worker 流；
  两者都不可用时仅允许 ≤ 500 MiB 的 Blob 回退。
- 任何文件解密失败都会中止整个 zip，不会产出缺失内容却看似完整的压缩包。
