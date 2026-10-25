# Tasks

## 1. 数据与命令行

- [ ] 1.1 迁移 `0006_admin.sql`（design D1）：用户状态与审批列、`is_admin`、`app_settings`、`admin_audit_log`、索引；已有用户设为 active；迁移测试覆盖“旧数据迁移后为 active”
- [ ] 1.2 `server admin grant|revoke|list <email>` 子命令（只连数据库，不需要 OSS/SMTP 配置），写审计日志（via=cli）；集成测试
- [ ] 1.3 配置项 `BACKUP_DEFAULT_QUOTA_BYTES`（默认 10 GiB）、`BACKUP_MAX_QUOTA_BYTES`（默认 10 TiB），启动期校验

## 2. 账号状态与准入

- [ ] 2.1 `AuthenticatedSession` 合并读取用户状态与管理员标志（单次查询）；非 active 用户访问写接口返回 `403 account_pending`；`/auth/me` 返回 status、status_reason、is_admin
- [ ] 2.2 登录：密码校验通过后对 suspended/rejected 返回对应错误与原因，不创建会话；错误密码/未知邮箱响应保持不可区分（测试对比响应）
- [ ] 2.3 注册模式（approval/open/closed）与每 IP 每小时 5 次注册限流；approval 模式下验证邮箱后入队 `admin_new_user` 邮件
- [ ] 2.4 为每个 user-accounts 新场景编写集成测试（待审核上传被拒且不预留配额、停用撤销会话并释放预留、关闭注册、注册限流）

## 3. 管理接口

- [ ] 3.1 管理员访问控制中间件（design D3）：非管理员 404、12 小时重新登录要求、禁止对自己停用/拒绝
- [ ] 3.2 `overview`、`users` 列表（游标分页、状态筛选、邮箱搜索转义）、`users/{id}` 详情
- [ ] 3.3 `approve`、`reject`、`suspend`、`reactivate`、`quota`、`revoke-sessions`：状态机、幂等、同事务审计、批准邮件 `account_approved`；suspend 中止上传会话并释放预留
- [ ] 3.4 `settings` 读写、`audit` 列表；OpenAPI 定义与前端类型生成
- [ ] 3.5 指标 `backup_users{status}`、`backup_admin_actions_total{action}`；日志不含邮箱与原因之外的敏感信息
- [ ] 3.6 集成测试覆盖 admin-console 全部场景（含非管理员 404、旧会话 reauth、额度低于已用量、重复批准幂等、审计旧值/新值）

## 4. 前端

- [ ] 4.1 等待审核页（pending 用户所有登录态路由）；登录页显示停用/拒绝原因；注册页处理 `registration_closed`
- [ ] 4.2 账号菜单“管理后台”入口（仅管理员）；`/admin` 路由与 404；`reauth_required` 时引导重新登录
- [ ] 4.3 管理后台：概览、用户（标签页筛选、搜索、表格、行内批准额度选择、详情抽屉、停用/拒绝原因与二次确认、修改额度、注销会话）、设置、审计日志；遵循 redesign-web-ui 视觉与动效规范；页面顶部注明管理员无法查看文件名或内容
- [ ] 4.4 组件测试 + Playwright：注册 → 等待审核页 → 管理员批准（额度 50 GB）→ 用户收到邮件并可上传；停用后登录显示原因；切换关闭注册；审计日志可见；截图脚本加入管理后台页面

## 5. 部署

- [ ] 5.1 `deploy/oss/production.env.example` 与 runbook 增加新配置项与“授予管理员”步骤；E2E 容器配置中通过命令行授予测试管理员
