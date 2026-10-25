# 阿里云 OSS 生产配置与上线 Runbook

本目录提供**可直接执行的模板**：RAM 最小权限策略、bucket CORS、生命周期、生产环境变量，
以及从零到 `/readyz` 就绪的逐步 runbook。

> **重要边界**
>
> 仓库与 CI 不包含任何真实阿里云凭据，也不会连接阿里云。创建 bucket、RAM 子账号、
> 配置 CORS/SSE/生命周期、保存 AccessKey、在测试 bucket 上验证等操作，必须由**用户本人**
> 在其阿里云账号中执行（见第 8 节与第 11 节）。
>
> `backend` 只把 OSS 当作密文对象存储：对象键固定为 `v1/{user_id}/{uuid}`，
> bucket 必须是私有读写的；所有浏览器访问都通过服务端生成的短期预签名 URL。

## 1. 模板文件

| 文件 | 用途 |
|---|---|
| [`ram-policy.json`](ram-policy.json) | RAM 自定义策略：只允许指定 bucket 的 `v1/*` 前缀执行 Put/Get/Delete/List/GetObjectMeta |
| [`cors.xml`](cors.xml) | bucket CORS：只允许正式前端源做 PUT/GET/HEAD，暴露 `ETag` |
| [`lifecycle.xml`](lifecycle.xml) | 清理未完成分片上传（卫生项），只作用于 `v1/` |
| [`production.env.example`](production.env.example) | 全部生产环境变量模板，含 `[SECRET]` 标记 |
| [`../../crates/server/tests/oss_smoke.rs`](../../crates/server/tests/oss_smoke.rs) | 真实 OSS 冒烟测试（`#[ignore]`，见第 9 节） |

## 2. 需要准备的资源

- 一个阿里云账号，以及一个**测试用** OSS bucket（不要直接拿生产 bucket 做首次验证）。
- 一个与 bucket 同地域的 VPC（若服务部署在阿里云 ECS/ACK；否则只能用公网 endpoint）。
- 一个 RDS PostgreSQL 实例（第 8 节运行 `server migrate`）。
- 正式前端域名与 TLS 证书（CORS 与 `BACKUP_PUBLIC_APP_URL` 都要用它）。
- 一个 SMTP 账号（推荐邮件推送 DirectMail），或先沿用内部 SMTP。

命名约定（下文占位符）：

| 占位符 | 示例 | 说明 |
|---|---|---|
| `<ACCOUNT_ID>` | `1234567890123456` | 阿里云账号 ID（RAM 策略 ARN 使用） |
| `<BUCKET_NAME>` | `backup-prod-xxxxxxxx` | 全局唯一 bucket 名 |
| `<REGION>` | `cn-hangzhou` | 地域 ID |
| `<OSS_REGION>` | `oss-cn-hangzhou` | OSS region 标识，注意带 `oss-` 前缀 |
| `<FRONTEND_ORIGIN>` | `https://backup.example.com` | 正式前端源，必须与浏览器地址栏完全一致 |

## 3. Bucket 配置（控制台步骤）

在 OSS 控制台创建 bucket 后，逐项确认：

1. **读写权限**：私有（Private）。不要开启公共读/公共读写，也不要绑定匿名访问。
2. **地域**：与后端部署地域一致，减少跨地域流量与延迟。
3. **版本控制**：**不开启**。版本由应用自己的 `file_versions` 管理；开启 OSS 版本控制后
   删除对象不会真正释放空间，与 design D15 的删除/配额语义冲突。
4. **服务器端加密**：开启“服务端加密”，算法选择 **SSE-OSS（AES256）**，作为纵深防御。
   即使 OSS 被未授权读取，内容仍是客户端密文；SSE 只是额外一层。
   - 这是 bucket 默认加密，不影响预签名 URL 的签名算法。
   - 不要改用 KMS 托管密钥（会引入额外权限与成本，design 未要求）。
5. **CORS**：粘贴 [`cors.xml`](cors.xml)，把 `https://backup.example.com` 替换为
   `<FRONTEND_ORIGIN>`。只保留 PUT/GET/HEAD；`ExposeHeader` 必须包含 `ETag`。
6. **生命周期**：合并 [`lifecycle.xml`](lifecycle.xml) 中的规则，清理 `v1/` 下
   超过 1 天的未完成分片上传。该配置是整体替换语义；若已有其他规则，先合并再提交。
7. **跨域/防盗链**：不要设置会拦截预签名 URL 的 Referer 白名单；预签名请求的
   Referer 是前端页面，但 OSS 直传不应依赖 Referer 校验。
8. **日志/清单**：按组织要求开启访问日志；日志中只会有对象键（UUID），不会有文件名。

### CORS 验证（用户本人执行）

配置完成后，用浏览器开发者工具或以下命令确认预检通过（把占位符替换为真实值）：

```bash
curl -i -X OPTIONS \
  'https://<BUCKET_NAME>.oss-cn-hangzhou.aliyuncs.com/v1/00000000-0000-4000-8000-000000000000/probe' \
  -H 'Origin: <FRONTEND_ORIGIN>' \
  -H 'Access-Control-Request-Method: PUT'
```

期望：HTTP 200/204，响应包含
`Access-Control-Allow-Origin: <FRONTEND_ORIGIN>`、`Access-Control-Allow-Methods` 中包含 `PUT`。
若返回 403，检查 CORS 的 AllowedOrigin 是否与页面源逐字符一致（含协议、端口、结尾斜杠）。

## 4. RAM 最小权限

### 4.1 策略

把 [`ram-policy.json`](ram-policy.json) 中的 `<ACCOUNT_ID>` 与 `<BUCKET_NAME>` 替换为真实值后，
在 RAM 控制台创建自定义策略（例如 `BackupOssV1`）。策略只授予：

- 对象级：`oss:PutObject`、`oss:GetObject`、`oss:DeleteObject`、`oss:GetObjectMeta`
- bucket 级：`oss:ListObjects`，并通过 `oss:Prefix` 条件限制为 `v1/*`

**不授予**：`oss:*`、`oss:DeleteBucket`、`oss:PutBucketAcl`、`oss:PutBucketPolicy`、
`oss:PutBucketCors`、`oss:PutBucketVersioning`、`oss:PutBucketEncryption` 等管理权限。
后端代码只需要 `v1/` 前缀下的对象读写、列举与删除（删除只由后台删除队列调用）。

策略中的资源 ARN 形式为：

```text
acs:oss:*:<ACCOUNT_ID>:<BUCKET_NAME>/v1/*
acs:oss:*:<ACCOUNT_ID>:<BUCKET_NAME>
```

### 4.2 子账号

1. 创建 RAM 用户（例如 `backup-oss-runtime`），只勾选“OpenAPI 调用访问”，
   **不要**勾选控制台登录。
2. 只附加 `BackupOssV1` 自定义策略；不要附加 `AliyunOSSFullAccess`。
3. 创建 AccessKey，把 ID/Secret 存入密钥管理服务或部署平台的 Secret；
   **不要**写进 Git、镜像、工单或聊天记录。
4. 记录 AccessKey 的创建时间，按组织策略定期轮换；轮换时先更新 Secret 再重启实例。

### 4.3 CLI 等价命令（可选，用户本人执行）

如果已安装并配置阿里云 CLI，可用以下命令创建策略与用户（把占位符替换为真实值）：

```bash
# 先替换 ram-policy.json 中的占位符
aliyun ram CreatePolicy \
  --PolicyName BackupOssV1 \
  --PolicyDocument "$(cat deploy/oss/ram-policy.json)" \
  --Description "Backup service: v1/* object read/write/delete/list only"

aliyun ram CreateUser --UserName backup-oss-runtime
aliyun ram AttachPolicyToUser \
  --PolicyType Custom --PolicyName BackupOssV1 --UserName backup-oss-runtime
aliyun ram CreateAccessKey --UserName backup-oss-runtime
```

最后一条命令会输出 AccessKeySecret；请直接在受控终端里导入密钥管理系统，不要粘贴到日志。
bucket/CORS/SSE/生命周期建议使用控制台配置；不同版本的 `ossutil`/CLI 命令参数可能变化，
执行前以 `ossutil help` 为准。

## 5. 环境变量清单

所有配置都来自 `BACKUP_` 前缀的环境变量（figment）。完整模板见
[`production.env.example`](production.env.example)。

“必填”列指该启动模式下必须存在；“密钥”列表示该值不得出现在日志/错误信息/镜像层中。

| 变量 | 含义 | 是否必填 | 示例值 | 是否密钥 |
|---|---|---|---|---|
| `BACKUP_BIND_ADDR` | HTTP API 与前端监听地址 | 否（默认 `127.0.0.1:8080`；容器用 `0.0.0.0:8080`） | `0.0.0.0:8080` | 否 |
| `BACKUP_METRICS_ADDR` | Prometheus 指标独立监听地址 | 否（默认 `127.0.0.1:9091`） | `0.0.0.0:9091` | 否 |
| `BACKUP_STATIC_DIR` | 前端构建产物目录 | 否（镜像内已设 `/app/web/dist`） | `/app/web/dist` | 否 |
| `BACKUP_DATABASE_URL` | PostgreSQL 连接串 | **是** | `postgres://backup:...@pgm-x.pg.rds.aliyuncs.com:5432/backup?sslmode=verify-full` | **是** |
| `BACKUP_DATABASE_MAX_CONNECTIONS` | 连接池上限（1–1024） | 否（默认 10） | `20` | 否 |
| `BACKUP_STORAGE_KIND` | 存储后端：`oss` 或 `s3` | **是** | `oss` | 否 |
| `BACKUP_STORAGE_BUCKET` | bucket 名 | **是** | `<BUCKET_NAME>` | 否 |
| `BACKUP_STORAGE_REGION` | OSS region（`oss` 模式必填） | `oss` 模式**是** | `oss-cn-hangzhou` | 否 |
| `BACKUP_STORAGE_INTERNAL_ENDPOINT` | 服务端 `stat`/`list`/`delete` 使用的内网 endpoint | **是** | `https://oss-cn-hangzhou-internal.aliyuncs.com` | 否 |
| `BACKUP_STORAGE_PUBLIC_ENDPOINT` | 只用于生成预签名 URL 的公网 endpoint | **是** | `https://oss-cn-hangzhou.aliyuncs.com` | 否 |
| `BACKUP_STORAGE_ACCESS_KEY_ID` | RAM AccessKey ID | **是** | `<ram-access-key-id>` | **是** |
| `BACKUP_STORAGE_SECRET_ACCESS_KEY` | RAM AccessKey Secret | **是** | `<ram-access-key-secret>` | **是** |
| `BACKUP_STORAGE_ALLOW_HTTP` | 是否允许 http endpoint | 否（默认 `false`；生产必须 `false`） | `false` | 否 |
| `BACKUP_JOBS_ENABLED` | 是否在本实例运行后台任务 | 否（默认 `true`） | `true` | 否 |
| `BACKUP_MAIL_MODE` | 邮件模式：`file` 或 `smtp` | **是（无默认值）** | `smtp` | 否 |
| `BACKUP_MAIL_DIR` | `file` 模式邮件目录 | 仅 `file` 模式（默认 `var/mail`） | `/var/lib/backup/mail` | 否 |
| `BACKUP_SMTP_HOST` | SMTP 主机 | `smtp` 模式**是** | `smtpdm.aliyun.com` | 否 |
| `BACKUP_SMTP_PORT` | SMTP 端口 | 否（按 TLS 模式取 `465`/`587`/`25`） | `465` | 否 |
| `BACKUP_SMTP_FROM` | 发件人地址 | `smtp` 模式**是** | `backup@example.com` | 否 |
| `BACKUP_SMTP_USERNAME` | SMTP 用户名 | TLS 模式**是**（与密码同时配置） | `<smtp-username>` | 是（凭据） |
| `BACKUP_SMTP_PASSWORD` | SMTP 密码 | TLS 模式**是**（与用户名同时配置） | `<smtp-password>` | **是** |
| `BACKUP_SMTP_TLS` | `starttls`、`implicit` 或 `none` | 否（默认 `starttls`） | `implicit` | 否 |
| `BACKUP_SMTP_ALLOW_INSECURE` | 显式允许明文/无认证 SMTP | 否（默认 `false`；生产必须 `false`） | `false` | 否 |
| `BACKUP_AUTH_PEPPER_VERSION` | 当前 pepper 版本号（正整数） | **是** | `1` | 否 |
| `BACKUP_AUTH_PEPPER` | 认证 HMAC 的 pepper，至少 32 字节 | **是** | `openssl rand -base64 48` 的输出 | **是** |
| `BACKUP_AUTH_OLD_PEPPERS` | 旧 pepper JSON：`{"版本":"值"}` | 否（轮换期才需要） | `{"0":"<old>"}` | **是** |
| `BACKUP_AUTH_FAKE_SALT_KEY` | prelogin 伪造盐密钥，至少 32 字节且不同于 pepper | **是** | 另一份 `openssl rand -base64 48` | **是** |
| `BACKUP_PUBLIC_APP_URL` | 邮件链接使用的前端外部地址 | 否（默认 `http://127.0.0.1:8080`；生产必须设为正式 https） | `https://backup.example.com` | 否 |
| `BACKUP_TRUSTED_PROXIES` | 可信代理 CIDR，逗号分隔 | 否（默认不信任任何代理） | `10.0.0.0/8,172.16.0.0/12` | 否 |
| `BACKUP_AUTH_RATE_LIMIT_PER_MINUTE` | 每 IP 每分钟认证请求上限 | 否（默认 60） | `60` | 否 |
| `BACKUP_USER_WRITE_RATE_LIMIT_PER_MINUTE` | 每用户每分钟写接口上限 | 否（默认 240） | `240` | 否 |

密钥类配置在 `Debug` 输出中固定为 `[REDACTED]`；缺失/非法时进程只输出配置项名称并以
非零码退出。

## 6. 内网 / 公网 endpoint 的区别

| 配置 | 谁访问 | 地址形态 | 说明 |
|---|---|---|---|
| `BACKUP_STORAGE_INTERNAL_ENDPOINT` | 服务端进程 | `https://oss-<region>-internal.aliyuncs.com` | 阿里云 VPC 内网地址，通常不解析公网；走内网可降低流量费用与延迟 |
| `BACKUP_STORAGE_PUBLIC_ENDPOINT` | 用户浏览器 | `https://oss-<region>.aliyuncs.com` | 只用于生成预签名 URL；CSP 实际追加的是 bucket 子域名 origin（见下） |

注意事项：

- 两个 endpoint 必须指向**同一个 bucket 所在地域**；否则预签名 URL 的 host 与 bucket
  不匹配，浏览器 PUT/GET 会失败。
- 内网 endpoint 只在同地域 VPC/ECS/ACK 中可解析。若后端部署在阿里云之外（例如自建机房），
  把 internal endpoint 也设为公网 endpoint；这不是最佳实践，但功能上可行。
- 预签名 URL 的签名基于公网 endpoint 生成；不要在前端再做 URL 重写。
- **CSP 的 `connect-src` 追加的是 bucket 子域名 origin，而不是 endpoint 本身**：
  OSS 预签名 URL 使用虚拟主机风格，实际 host 是
  `https://<bucket>.oss-<region>.aliyuncs.com`（带端口时保留端口）。服务端从
  `BACKUP_STORAGE_PUBLIC_ENDPOINT` 的 host 自动拼接 bucket，得到并写入 CSP 的正是这个 origin。
  因此不要填 `https://<bucket>.oss-<region>.aliyuncs.com`——启动期会拒绝带 bucket 子域名的
  配置，避免变成 `bucket.bucket.host`。这里选择拒绝而不是自动去掉前缀，是因为自定义域名
  无法可靠判断该前缀是否真的是 bucket 子域名，静默去重会掩盖配置错误。
- 切换 endpoint 后必须重启 `server serve` 使 CSP 的 `connect-src` 更新。若浏览器控制台报
  CSP 拦截 `https://<bucket>.oss-<region>.aliyuncs.com`，先确认填的是不带 bucket 的标准
  endpoint；不要用 `https://*.aliyuncs.com` 通配符放宽 CSP。
- `BACKUP_STORAGE_ALLOW_HTTP=false` 时两个 endpoint 都必须是 https。

## 7. 逐步 Runbook（用户本人在阿里云执行）

### 步骤 0：生成密钥并准备数据库

```bash
# 生成 pepper / fake salt key（不要复用同一个值）
openssl rand -base64 48
openssl rand -base64 48
```

在 RDS 创建数据库与最小权限账号；记录连接串（含 TLS 参数）。

### 步骤 1：创建 bucket

控制台 → 对象存储 OSS → Bucket 列表 → 创建 Bucket：

- Bucket 名称：`<BUCKET_NAME>`（全局唯一）
- 地域：与后端相同
- 读写权限：**私有**
- 版本控制：**不开**
- 服务端加密：**SSE-OSS（AES256）**
- 创建后进入“数据安全”确认加密已开启。

### 步骤 2：配置 CORS 与生命周期

1. Bucket → 数据安全 → 跨域设置 → 创建规则，粘贴 [`cors.xml`](cors.xml)，
   把来源替换为 `<FRONTEND_ORIGIN>`。
2. Bucket → 数据管理 → 生命周期 → 创建规则，合并 [`lifecycle.xml`](lifecycle.xml)
   的“未完成分片上传 1 天清理”规则。
3. 用第 3 节的 `curl OPTIONS` 验证 CORS。

### 步骤 3：创建 RAM 用户与最小权限策略

按第 4 节替换策略占位符、创建策略、创建只带 OpenAPI 访问的 RAM 用户、附加策略、
创建 AccessKey，并立即把 AccessKey 写入密钥管理系统。

### 步骤 4：填写环境变量

```bash
sudo install -m 600 -o deploy -g deploy \
  deploy/oss/production.env.example /etc/backup/backup.env
sudo editor /etc/backup/backup.env   # 替换所有占位符
```

逐项核对第 5 节表格。生产至少确认：

- `BACKUP_STORAGE_KIND=oss`
- `BACKUP_STORAGE_ALLOW_HTTP=false`
- `BACKUP_MAIL_MODE=smtp`、`BACKUP_SMTP_TLS=implicit`（或 `starttls`）
- `BACKUP_SMTP_ALLOW_INSECURE=false`
- `BACKUP_PUBLIC_APP_URL=https://<正式域名>`
- `BACKUP_AUTH_PEPPER`、`BACKUP_AUTH_FAKE_SALT_KEY` 各不相同且 ≥32 字节
- `BACKUP_TRUSTED_PROXIES` 只包含实际代理网段

### 步骤 5：启动前先迁移

镜像已把迁移嵌入 server 二进制；发布流程必须先执行 `migrate`：

```bash
# 方式 A：直接运行镜像（推荐；镜像默认非 root）
docker run --rm --env-file /etc/backup/backup.env backup-server:<tag> migrate

# 方式 B：在已有容器/主机内运行
/usr/local/bin/server migrate
```

`migrate` 只读取 `BACKUP_DATABASE_URL`，不需要 OSS/SMTP 配置；失败时进程非零退出。
迁移完成后再部署新版本 `serve`（可 ≥2 实例，置于负载均衡后）。

### 步骤 6：启动服务并验证

```bash
docker run -d --name backup-server \
  --env-file /etc/backup/backup.env \
  -p 8080:8080 -p 9091:9091 \
  backup-server:<tag> serve
```

验证：

```bash
# 1) 存活
curl -fsS https://<正式域名>/healthz

# 2) 就绪：数据库 + 迁移版本 + OSS 可访问（缺一不可）
curl -fsS https://<正式域名>/readyz

# 3) 首页（应返回 index.html）
curl -fsS https://<正式域名>/ | grep -q '<div id="root">'

# 4) 静态资源 MIME / 缓存头
curl -fsSI https://<正式域名>/assets/<某个带哈希的 js>
```

`/readyz` 返回 503 时，响应里的 `database` / `migrations` / `storage` 字段会指出哪一项失败。
常见原因：RDS 安全组未放行、OSS 内网 endpoint 在 VPC 外不可解析、RAM 策略未覆盖 `v1/*`。

### 步骤 7：真实 OSS 冒烟测试

见第 9 节；首次配置完成后由用户本人在测试 bucket 上跑一次。

### 步骤 8：上线后检查

- 注册一个测试账号，完成验证邮件、登录、上传/下载/删除。
- 确认 OSS 对象键为 `v1/{uuid}/{uuid}`，bucket 中看不到文件名。
- 确认服务端日志没有 `Authorization`、Cookie、预签名查询串或请求体。
- 观察 `/metrics` 的 `backup_object_deletion_queue_*` 等指标。

## 8. 回滚与密钥轮换

- **回滚镜像**：重新部署上一版本镜像。迁移只做向前兼容的增量变更，旧版本可以在新 schema
  上运行（service-operations spec）。
- **AccessKey 轮换**：在 RAM 创建新 AccessKey → 更新 Secret → 滚动重启所有实例 →
  确认 `/readyz` 正常 → 禁用/删除旧 AccessKey。
- **pepper 轮换**：把当前 pepper 移入 `BACKUP_AUTH_OLD_PEPPERS`，设置新
  `BACKUP_AUTH_PEPPER_VERSION` 与 `BACKUP_AUTH_PEPPER`，滚动重启；旧版本保留至少一个
  登录周期，确认无旧版本请求后再移除。
- **不要轮换** `BACKUP_AUTH_FAKE_SALT_KEY` 之外的任何客户端加密材料：MK、节点密钥、
  恢复密钥都由客户端管理，服务端轮换不会改变已有密文。

## 9. 真实 OSS 冒烟测试（9.3）

测试代码在 [`crates/server/tests/oss_smoke.rs`](../../crates/server/tests/oss_smoke.rs)，
默认 `#[ignore]`，只在显式指定时运行。它使用**测试 bucket**，流程为：

1. 用 `Storage` 的 OSS 配置生成预签名 PUT URL；
2. HTTP PUT 一个随机测试对象；
3. 用预签名 GET URL 下载并逐字节比对；
4. 删除对象；
5. `stat` 确认对象已不存在（失败时也会尽力清理）。

所需环境变量：

| 变量 | 示例 | 密钥 |
|---|---|---|
| `OSS_SMOKE_BUCKET` | `backup-smoke-xxxxxxxx` | 否 |
| `OSS_SMOKE_REGION` | `oss-cn-hangzhou` | 否 |
| `OSS_SMOKE_INTERNAL_ENDPOINT` | `https://oss-cn-hangzhou.aliyuncs.com`（GitHub runner 无 VPC，填公网） | 否 |
| `OSS_SMOKE_PUBLIC_ENDPOINT` | `https://oss-cn-hangzhou.aliyuncs.com` | 否 |
| `OSS_SMOKE_ACCESS_KEY_ID` | RAM AccessKey ID | **是** |
| `OSS_SMOKE_SECRET_ACCESS_KEY` | RAM AccessKey Secret | **是** |

运行：

```bash
OSS_SMOKE_BUCKET='...' \
OSS_SMOKE_REGION='oss-cn-hangzhou' \
OSS_SMOKE_INTERNAL_ENDPOINT='https://oss-cn-hangzhou.aliyuncs.com' \
OSS_SMOKE_PUBLIC_ENDPOINT='https://oss-cn-hangzhou.aliyuncs.com' \
OSS_SMOKE_ACCESS_KEY_ID='...' \
OSS_SMOKE_SECRET_ACCESS_KEY='...' \
SQLX_OFFLINE=true \
cargo test -p server --test oss_smoke -- --ignored --nocapture
```

凭据缺失时测试会打印“跳过 OSS 冒烟测试：缺少 …”并以成功结束——这是仓库中**唯一**
允许因缺少外部依赖而跳过的测试。GitHub Actions 的夜间 workflow
[`oss-smoke.yml`](../../.github/workflows/oss-smoke.yml) 从 Secrets 读取同样的值；
未配置 Secrets 时 workflow 以“跳过”结束，不会失败。

## 10. 已知未验证项

- 本仓库的自动化环境没有真实阿里云账号，因此 9.2 的“在测试 bucket 上配置一遍”和
  9.3 的“对真实 OSS 跑通一次”需要在用户环境中执行后才能勾选任务。
- `ram-policy.json` 的 `oss:Prefix` 条件语法以阿里云当前 RAM 文档为准；若控制台策略校验
  不接受该条件，请先用控制台策略模拟器验证 `ossutil ls oss://<bucket>/v1/`，不要直接
  放宽为 `oss:*`。
- 不同版本 `ossutil` / 阿里云 CLI 的 bucket CORS、加密、生命周期子命令可能变化；
  本 runbook 的 bucket 配置以控制台步骤为准。
