# Spec Delta

## Purpose

为账号引入状态与审批：注册后需管理员批准才能使用服务，并支持停用与拒绝，以防止恶意注册占用资源。

## ADDED Requirements

### Requirement: 注册后待审批
在“需要审批”注册模式下，新注册账号 SHALL 处于 `pending` 状态：可以验证邮箱和登录，但在被批准前 MUST NOT 能创建文件夹、上传文件或进行任何写入操作（返回 `403 account_pending`）。系统 SHALL 在用户验证邮箱后通知管理员，在账号被批准后通知用户。

#### Scenario: 待审核用户尝试上传
- **WHEN** 待审核用户登录后请求创建上传会话
- **THEN** 服务端返回 `403 account_pending`，不创建会话、不预留配额

#### Scenario: 待审核用户看到等待页面
- **WHEN** 待审核用户登录
- **THEN** 页面显示“正在等待管理员审核”的说明与退出登录入口，而不是文件页

#### Scenario: 批准后通知
- **WHEN** 管理员批准一个待审核账号
- **THEN** 该账号状态变为 `active`、获得设定的额度，并收到“账号已开通”邮件

### Requirement: 已有账号保持可用
引入账号状态时，迁移前已存在的账号 SHALL 全部被设为 `active`，其使用不受影响。

#### Scenario: 迁移后的老用户
- **WHEN** 迁移完成后老用户登录并上传文件
- **THEN** 一切照常，不需要审批

### Requirement: 停用与拒绝
被停用或被拒绝的账号 SHALL NOT 能登录；只有在密码校验通过后，登录接口才返回 `account_suspended` 或 `account_rejected` 以及管理员填写的原因，对未知邮箱与错误密码的响应 MUST 与现有行为不可区分。停用 MUST 撤销该账号的全部会话并释放进行中上传的配额预留，已有数据保留。

#### Scenario: 停用后登录
- **WHEN** 被停用用户输入正确的密码登录
- **THEN** 登录失败并显示停用原因，没有创建会话

#### Scenario: 停用时撤销会话
- **WHEN** 管理员停用一个在线用户
- **THEN** 该用户下一次请求返回 401，进行中的上传会话被中止且配额预留被释放

#### Scenario: 不泄露账号状态
- **WHEN** 有人用被停用账号的邮箱和错误密码尝试登录
- **THEN** 响应与“邮箱或密码错误”完全相同

### Requirement: 注册模式
系统 SHALL 支持三种注册模式：需要审批（默认）、开放注册、关闭注册。关闭注册时注册接口 MUST 返回 `403 registration_closed`；开放注册时新账号直接为 `active`。

#### Scenario: 关闭注册
- **WHEN** 注册模式为“关闭注册”时有人提交注册
- **THEN** 返回 `registration_closed`，没有创建账号，注册页显示“暂不开放注册”

### Requirement: 注册频率限制
同一客户端 IP 每小时成功注册的账号数 SHALL NOT 超过 5 个，超出时返回 `429 rate_limited`。

#### Scenario: 批量注册
- **WHEN** 同一 IP 在一小时内尝试注册第 6 个账号
- **THEN** 返回 `429 rate_limited`，没有创建账号
