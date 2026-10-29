# 生产部署（单台云主机）

本目录是线上环境（阿里云 ECS + 阿里云 OSS）的完整部署方式：一台 Linux 主机运行
PostgreSQL 与应用容器，宿主机上的 Caddy 提供 HTTPS，文件密文存放在 OSS。
OSS 的 bucket、RAM 最小权限与 CORS 配置见 [deploy/oss/README.md](../oss/README.md)。

```
浏览器 ──HTTPS──▶ Caddy（宿主机 443） ──▶ server 容器（127.0.0.1:8080） ──▶ PostgreSQL 容器
   │                                          │
   └──────────── 预签名 URL 直传密文 ─────────▶ 阿里云 OSS ◀── 内网 endpoint
```

| 文件 | 用途 |
|---|---|
| `compose.yml` | PostgreSQL + server 两个容器（小内存调优，固定网段 172.30.0.0/24） |
| `backup.env.example` | 应用配置模板（OSS、SMTP、限流、额度） |
| `Caddyfile.template` | HTTPS 反向代理模板：域名或公网 IP 两种模式 |
| `install.sh` | 首次安装：swap、Docker、Caddy、目录、随机密钥、备份定时任务 |
| `release.sh` | **一键发布**：本机构建镜像 → 上传 → 服务器升级（失败自动回滚） |
| `upgrade.sh` / `rollback.sh` | 服务器上的升级 / 回滚 |
| `db-backup.sh` / `restore-db.sh` | 数据库备份（每天 03:30，保留 14 天）/ 恢复 |
| `get-link.sh` | 未配置 SMTP 时取验证邮件链接 |

服务器上的目录 `/opt/backup`（权限 700）：`compose.yml`、脚本、`.env`（数据库密码与当前镜像）、
`backup.env`（应用配置与密钥）、`db-backups/`、`.previous-image`（回滚用）。
`.env` 与 `backup.env` 只存在服务器上，权限 600，永远不进仓库。

## 前置条件

- Ubuntu 22.04/24.04（或 Debian 12），≥ 1 GiB 内存（不足 2 GiB 时自动加 2 GiB swap），≥ 10 GiB 磁盘。
- 安全组放行 TCP 80、443（以及 SSH）。应用与数据库端口不对外。
- 开发机：git、docker、ssh（Windows 用 WSL），能用 SSH 密钥以 root 登录服务器。
- 访问地址二选一：
  - **域名**：解析到服务器公网 IP；Caddy 自动申请证书。中国大陆服务器需要 ICP 备案。
  - **公网 IP**：Caddy 用 Let's Encrypt 短期 IP 证书（约 6 天，自动续期）。适合少数人使用；
    中国大陆服务器对公网提供网站服务按规定同样需要备案，有被拦截的风险。

## 首次安装

```bash
# 开发机（仓库根目录）
scp -r deploy/server root@<host>:/root/backup-deploy
ssh root@<host> 'SITE_HOST=<域名或公网IP> bash /root/backup-deploy/install.sh'
```

然后在服务器上编辑 `/opt/backup/backup.env`：

1. 填写 OSS：`BACKUP_STORAGE_BUCKET`、`REGION`、两个 endpoint、`ACCESS_KEY_ID`、`SECRET_ACCESS_KEY`
   （用只授予 `v1/*` 权限的 RAM 子账号，见 deploy/oss/README.md）。
2. 在 OSS bucket 的跨域规则里加入 `https://<SITE_HOST>`。
3. 可选：配置 SMTP（模板里有 QQ 邮箱示例；`BACKUP_SMTP_PASSWORD` 是邮箱的 SMTP 授权码）。
   不配置时邮件写入文件，用 `/opt/backup/get-link.sh <邮箱>` 取验证链接。

第一次发布（见下一节）后授予管理员：

```bash
ssh root@<host> 'cd /opt/backup && docker compose exec -T server /usr/local/bin/server admin grant <你的邮箱>'
```

该邮箱需要先在网站上注册。默认注册模式是“需要审批”：新用户需要管理员在“管理后台 → 用户 → 待审核”批准。

## 发布 / 升级（一键）

```bash
# 开发机，在 WSL/Linux 中、仓库根目录
SERVER=root@<host> SSH_KEY=~/.ssh/<key> deploy/server/release.sh
# 国内网络构建时访问 GitHub 慢或失败：加 BUILD_PROXY=http://127.0.0.1:10808
# 发布指定提交：加 REV=<commit>
```

`release.sh` 只打包**已提交**的代码，镜像名为 `backup-server:<短 SHA>`。服务器上的 `upgrade.sh` 会：
备份数据库 → 记录当前镜像 → 切换镜像（容器启动时自动执行数据库迁移）→ 等待 `/readyz`；
90 秒内未就绪会自动回滚到之前的镜像并报错退出。

发布前建议确认该提交在 GitHub CI 上全部通过。

## 回滚

```bash
ssh root@<host> /opt/backup/rollback.sh                       # 回到上一个版本
ssh root@<host> /opt/backup/rollback.sh backup-server:<sha>    # 回到指定版本
ssh root@<host> 'docker images backup-server'                  # 查看本机已有的版本
```

数据库迁移只做向前兼容的增量变更，旧版本镜像可以直接在新 schema 上运行，回滚不需要恢复数据库。
旧镜像会一直留在服务器上，磁盘紧张时用 `docker image rm backup-server:<sha>` 删除不再需要的版本。

## 数据库备份与恢复

- 每天 03:30 自动备份到 `/opt/backup/db-backups/`，保留 14 天；每次升级前也会自动备份一次。
- 手动备份：`ssh root@<host> /opt/backup/db-backup.sh`
- 建议定期把备份复制到另一台机器或另一个 OSS bucket（例如开发机上
  `scp root@<host>:/opt/backup/db-backups/backup-*.dump ./`）。
- 恢复（会覆盖当前数据库，恢复前会再自动备份一次当前数据库）：

  ```bash
  ssh root@<host> /opt/backup/restore-db.sh /opt/backup/db-backups/backup-<时间>.dump --yes
  ```

数据库里只有账号与文件结构等元数据（文件名本身是密文）；文件内容在 OSS 中，不在这个备份里。

## 日常运维

```bash
ssh root@<host> 'cd /opt/backup && docker compose ps'                 # 容器状态
ssh root@<host> 'cd /opt/backup && docker compose logs --tail 100 server'
curl -fsS https://<host>/readyz                                         # 数据库、迁移、OSS 三项检查
```

修改 `backup.env` 后需要重启应用：`cd /opt/backup && docker compose up -d server`。

## 常见问题

| 现象 | 原因与处理 |
|---|---|
| 浏览器访问 `https://<IP>` 时 TLS 握手失败 | Caddyfile 缺少 `default_sni <IP>`（云主机公网 IP 经 NAT 映射）。重新运行 install.sh 渲染模板 |
| 上传时浏览器报 CORS 错误 | OSS bucket 跨域规则里没有当前访问地址（协议 + 主机 + 端口必须完全一致） |
| `/readyz` 中 storage 失败 | OSS endpoint/地域与 bucket 不一致，或服务器不在同地域 VPC 却用了内网 endpoint |
| 构建镜像时下载 wasm-pack/依赖超时 | 设置 `BUILD_PROXY`；构建容器需要 `--network host` 才能用宿主机代理 |
| 服务器拉取镜像失败 | 服务器访问不了 Docker Hub 属正常现象：镜像由 release.sh 在开发机构建后上传 |
| 内存不足、容器被杀 | 确认 swap 已开启（`swapon --show`）；`compose.yml` 中的 `mem_limit` 可按主机内存调高 |
| 新用户反馈收不到验证邮件 | 检查 SMTP 配置与授权码；让对方看垃圾邮件箱；临时可用 get-link.sh（file 模式） |
