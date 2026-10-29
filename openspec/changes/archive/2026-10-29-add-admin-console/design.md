# Design

## Context

- `users` 已有 `quota_bytes`（默认 10 GiB）、`used_bytes`、`reserved_bytes`、`email_verified_at`、`failed_logins/locked_until`；`sessions` 有 `created_at/last_seen_at/revoked_at/ip/user_agent`。
- 上传接口已在邮箱未验证时返回 `403 email_not_verified`；认证由 `AuthenticatedSession` 提取器完成；所有写接口有每用户限流，认证接口有每 IP 限流。
- 邮件经 `email_outbox` 发出（令牌在发送时生成，D13）；后台任务、对象删除队列已存在。
- 端对端加密：服务端没有文件名与内容。管理后台只能展示服务端本来就有的元数据（bootstrap `client-encryption` “服务端可见信息边界”）。

## Goals / Non-Goals

**Goals：** 注册后需批准才能使用；按用户管理额度与状态；总体统计；管理操作可审计；管理员身份无法通过网页获得。

**Non-Goals：** 删除账号及其全部数据（不可逆、涉及对象清理，后续单独设计；现阶段用“停用”）；多级角色/权限细分；管理员查看任何文件信息（加密使然，也不应提供）；邀请码；人机验证（CAPTCHA）。

## Decisions

### D1 数据模型（迁移 0006，只增不改）

- `users.status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','active','suspended','rejected'))`；迁移中把所有已存在用户设为 `active`。
- `users.is_admin BOOLEAN NOT NULL DEFAULT false`；`users.status_changed_at TIMESTAMPTZ`、`users.status_reason TEXT`（≤ 500 字，管理员填写的原因，展示给用户）、`users.approved_at TIMESTAMPTZ`、`users.approved_by UUID`（复合外键不适用，普通外键到 `users(id)`，`ON DELETE SET NULL`）。
- `app_settings(key TEXT PRIMARY KEY, value JSONB NOT NULL, updated_at, updated_by)`：`registration_mode`（`approval` 默认 / `open` / `closed`）、`default_quota_bytes`（默认取配置 `BACKUP_DEFAULT_QUOTA_BYTES`，缺省 10 GiB）。
- `admin_audit_log(id BIGSERIAL, admin_id UUID NOT NULL, action TEXT NOT NULL, target_user_id UUID, details JSONB NOT NULL DEFAULT '{}', ip TEXT NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now())`，只追加；`details` 只放非敏感参数（额度数值、原因、旧值/新值），不放令牌或任何密钥材料。
- 索引：`users(status, created_at)`，`admin_audit_log(created_at DESC)`、`(target_user_id, created_at DESC)`。

### D2 状态与准入

| 状态 | 能否登录 | 能否读写文件 | 说明 |
|---|---|---|---|
| pending | 能 | 否（写接口返回 `403 account_pending`；列目录等读接口照常，其实只有空根目录） | 前端显示“等待审核”页 |
| active | 能 | 能 | — |
| suspended | 否（密码正确后返回 `403 account_suspended` 与原因） | 否 | 停用时撤销全部会话，数据保留 |
| rejected | 否（密码正确后返回 `403 account_rejected` 与原因） | 否 | — |

- 状态检查放在 `AuthenticatedSession` 提取器：每次请求读取用户状态（与会话校验同一查询，不额外往返）；非 active 用户访问写接口一律 403。允许 pending 用户访问：`/auth/me`、`/auth/logout`、`/auth/sessions*`、`/auth/change-password`、`/auth/rotate-recovery-key`、只读列表。
- 登录接口：只有在密码校验通过后才返回 suspended/rejected 的具体错误，保持对未知邮箱/错误密码的不可区分（不泄露账号是否存在）。
- `/auth/me` 增加 `status`、`status_reason`、`is_admin` 字段。
- 注册：`registration_mode=closed` 时返回 `403 registration_closed`；`open` 时新用户为 `active` 并使用默认额度；`approval` 时为 `pending`，`quota_bytes` 设为默认额度但因状态无法使用。
- 每 IP 每小时最多 5 次成功注册（内存令牌桶，键为可信代理解析后的客户端 IP；与现有每 IP 认证限流叠加）。

### D3 管理员身份与会话要求

- 只能通过 `server admin grant <email>` / `server admin revoke <email>` / `server admin list` 设置（直接连数据库，写审计日志 `admin_id` 为系统哨兵值并记录 `via=cli`）。接口层没有任何修改 `is_admin` 的路径。
- 管理接口要求：会话有效、用户 `is_admin=true` 且 `status=active`、会话 `created_at` 在 12 小时内（否则 `403 reauth_required`，前端引导重新登录）。非管理员访问 `/api/v1/admin/*` 一律 `404`（不暴露接口存在）。
- 管理员不能停用或拒绝自己的账号（`400 cannot_modify_self`），以免把唯一的管理员锁在门外；修改自己的额度允许。

### D4 管理接口（`/api/v1/admin`，全部带 OpenAPI 定义）

- `GET /overview`：各状态用户数、待审核数、总 used/reserved/quota、近 7 天注册数、`email_outbox` 待发数、`object_deletions` 待处理数、`/readyz` 结果、服务版本。
- `GET /users?status=&q=&cursor=`：游标分页（按 `created_at DESC, id`），`q` 为邮箱子串（参数化 `ILIKE`，转义 `%`/`_`）。每项：id、email、status、is_admin、created_at、email_verified_at、approved_at、最近活动（未撤销会话的最大 `last_seen_at`）、used/reserved/quota、节点数（文件/文件夹）、版本数、活动会话数。
- `GET /users/{id}`：上述字段 + 最近 20 个会话（创建时间、最近使用、IP、User-Agent、是否撤销）+ 针对该用户的最近 20 条审计记录。
- `POST /users/{id}/approve {quota_bytes?}`：pending/rejected → active，设定额度（缺省取默认额度），发送“账号已开通”邮件。
- `POST /users/{id}/reject {reason}`：pending → rejected，撤销会话。
- `POST /users/{id}/suspend {reason}`：active/pending → suspended，撤销全部会话、取消进行中的上传会话并释放预留（复用现有过期清理逻辑）。
- `POST /users/{id}/reactivate`：suspended/rejected → active。
- `PUT /users/{id}/quota {quota_bytes}`：0 ≤ 值 ≤ `BACKUP_MAX_QUOTA_BYTES`（默认 10 TiB）；低于已用量时允许但提示（用户无法继续上传，已有数据不受影响）。在锁定用户行的事务中更新。
- `POST /users/{id}/revoke-sessions`。
- `GET /settings`、`PUT /settings {registration_mode, default_quota_bytes}`。
- `GET /audit?cursor=&target_user_id=`。
- 所有状态变更接口幂等：目标状态已达成时返回 200 且不重复发邮件、不重复写审计。所有写操作在同一事务内写 `admin_audit_log`。
- 所有 SQL 参数化；访问不存在的用户 404。

### D5 邮件

- `admin_new_user`：用户在 approval 模式下完成邮箱验证时，入队给每位管理员（内容：新用户邮箱、注册时间、后台链接 `{public_app_url}/admin/users?status=pending`）。
- `account_approved`：批准时发给用户（内容：已开通、额度、登录链接）。
- 拒绝/停用不发邮件（避免向滥用者提供反馈信号），用户登录时看到原因。

### D6 前端

- **等待审核页**：pending 用户登录后所有登录态路由显示该页（插图 + “你的账号正在等待管理员审核，开通后会发邮件通知你”+ 退出登录、安全与隐私链接）。suspended/rejected 登录失败时在登录页显示原因。
- **管理后台** `/admin`（沿用 AppShell：侧边栏切换为“概览 / 用户 / 设置 / 审计日志”与“返回我的文件”；顶部注明“管理员无法查看用户的文件名或内容”）：
  - 概览：统计卡片（待审核数醒目，点击直达待审核列表）、总用量条、队列积压、健康状态。
  - 用户：状态标签页（待审核 N / 正常 / 已停用 / 已拒绝 / 全部）、邮箱搜索、表格（邮箱、状态徽标、注册时间、最近活动、用量/额度进度条、操作）；待审核行内直接“批准”（弹出额度选择：默认值、5/10/50/100 GB、自定义）与“拒绝”；行点击打开右侧详情抽屉（会话列表、审计记录、修改额度、停用/恢复、注销会话）。危险操作（停用、拒绝）需填写原因并二次确认。
  - 设置：注册模式单选（附说明）、默认额度。
  - 审计日志：时间线列表，可按用户筛选。
- 账号菜单中仅管理员显示“管理后台”。非管理员访问 `/admin` 显示 404 页。
- 视觉、文案、动效遵循 redesign-web-ui（D2/D10、口语化文案）；表格行数多时虚拟滚动。

### D7 可观测性

- 指标：`backup_users{status}`（定时刷新）、`backup_admin_actions_total{action}`。日志只记录管理操作的 action、admin_id、target_user_id；邮箱与管理员填写的原因不写入日志（原因只存在审计表中）。

## Risks / Trade-offs

- **每次请求多读用户状态** → 与会话查询合并为一次 JOIN，无额外往返。
- **管理员账号被盗即可停用他人** → 管理员只能命令行授予；管理操作要求 12 小时内重新登录；全部审计；管理员也无法读取任何用户文件（E2EE）。
- **注册限流基于内存** → 单实例部署足够；多实例时为每实例独立计数（可接受，审批仍是最后一道关）。
- **pending 用户占用少量资源**（账号行与根节点）→ 可接受；自动清理长期未验证账号留待后续。

## Migration Plan

1. 部署新镜像，`server migrate` 应用 0006（现有用户全部 active，行为不变）。
2. `docker compose exec server server admin grant tlj978626571@gmail.com`。
3. 管理员重新登录后即可在账号菜单进入“管理后台”。默认注册模式为“需要审批”，此后新注册用户需批准。
回滚：旧镜像可在新 schema 上运行（新增列都有默认值，旧代码忽略），但会失去审批限制。
