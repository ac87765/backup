# Spec Delta

## Purpose

定义服务在生产环境中可运维、可观测、可安全部署所需的行为：配置校验、健康检查、日志与指标、安全响应头、后台任务的可靠执行与优雅停机。

## ADDED Requirements

### Requirement: 启动配置校验
服务 SHALL 从环境变量（可选配置文件）读取配置，启动时校验所有必填项与取值范围，任一不合法时 MUST 以非零退出码退出并输出指明错误项的日志（不输出密钥值）。

#### Scenario: 缺少 OSS 凭据
- **WHEN** 启动时未设置对象存储访问凭据
- **THEN** 进程立即退出并记录缺失的配置项名称

### Requirement: 健康检查
服务 SHALL 提供存活检查 `/healthz`（进程可响应即成功）与就绪检查 `/readyz`（数据库可连接、迁移版本匹配、对象存储可访问时成功）。

#### Scenario: 数据库不可用
- **WHEN** 数据库连接失败
- **THEN** `/readyz` 返回 503，`/healthz` 仍返回 200

### Requirement: 数据库迁移
数据库结构 SHALL 由版本化迁移管理。服务启动时若数据库迁移版本落后于代码，MUST 拒绝开始服务（除非以显式的迁移命令启动）；迁移 MUST 只向前兼容地演进，不得在同一发布中删除仍被旧版本读取的列。

#### Scenario: 未执行迁移即启动
- **WHEN** 新版本服务连接到尚未迁移的数据库
- **THEN** 服务记录错误并退出，不对外提供服务

### Requirement: 结构化日志与请求追踪
服务 SHALL 输出 JSON 结构化日志，每个请求分配请求 ID 并在响应头 `X-Request-Id` 与错误响应体中返回。日志 MUST NOT 包含会话令牌、认证密钥、包裹密钥、加密元数据、预签名 URL 签名或邮箱以外的个人数据。

#### Scenario: 排查用户报错
- **WHEN** 用户提供错误提示中的请求 ID
- **THEN** 运维可通过该 ID 检索到完整请求日志链路

#### Scenario: 日志脱敏
- **WHEN** 对日志进行敏感信息扫描测试
- **THEN** 未发现令牌、密钥或预签名签名

### Requirement: 指标
服务 SHALL 在独立端口暴露 Prometheus 指标，至少包括：HTTP 请求数与延迟（按路由与状态码）、数据库连接池使用、上传会话按状态计数、删除队列积压长度与最老任务年龄、后台任务执行次数与失败次数。

#### Scenario: 删除队列积压
- **WHEN** 对象存储持续故障导致删除队列积压
- **THEN** 积压长度与最老任务年龄指标持续上升，可据此告警

### Requirement: 统一错误响应
所有 API 错误 SHALL 返回 JSON 体 `{code, message, request_id}`，`code` 为稳定的机器可读字符串。服务端内部错误 MUST 只返回通用信息，详细原因只写入日志。

#### Scenario: 内部错误
- **WHEN** 处理请求时发生未预期的数据库错误
- **THEN** 客户端收到 500 与 `code=internal_error`，响应中不包含 SQL 或堆栈信息

### Requirement: 安全响应头
服务 SHALL 为所有页面设置严格 CSP（仅允许同源脚本与 WASM 执行、禁止内联脚本、`connect-src` 仅限同源与对象存储域名）、HSTS、`X-Content-Type-Options: nosniff`、`Referrer-Policy: no-referrer`、`frame-ancestors 'none'`。

#### Scenario: 注入的内联脚本
- **WHEN** 页面中出现内联 `<script>`
- **THEN** 浏览器根据 CSP 拒绝执行

### Requirement: 请求体与速率限制
服务 SHALL 对 API 请求体大小设置上限（文件内容不经过服务端，因此上限为 1 MiB 量级），并对已认证用户的写接口设置每用户速率上限，超出返回 429。

#### Scenario: 超大请求体
- **WHEN** 客户端发送 10 MiB 的 JSON 请求
- **THEN** 系统返回 413，不解析请求体

### Requirement: 可靠的后台任务
后台任务（删除队列、会话过期、版本清理、回收站清理、孤儿巡检、过期会话与令牌清理）SHALL 在多实例部署下不重复执行同一工作项，任务失败 MUST 记录并在之后重试，单个工作项的失败 MUST NOT 阻塞其他工作项。

#### Scenario: 两个实例同时运行
- **WHEN** 两个服务实例同时处理删除队列
- **THEN** 每个队列项只被一个实例领取

#### Scenario: 实例在处理中崩溃
- **WHEN** 实例领取任务后崩溃
- **THEN** 领取超时后该任务被其他实例重新领取并完成

### Requirement: 优雅停机
收到 SIGTERM 时服务 SHALL 停止接受新连接、等待进行中请求完成（最长 30 秒）、停止领取新的后台任务并让当前任务完成或安全放弃，然后退出。

#### Scenario: 滚动发布
- **WHEN** 部署系统在发布时向旧实例发送 SIGTERM
- **THEN** 进行中的 API 请求正常完成，客户端不会看到连接被重置
