# Spec Delta

## ADDED Requirements

### Requirement: iOS 文件操作
iOS 客户端 SHALL 提供与桌面客户端一致的文件管理（浏览、新建、重命名、移动、删除到回收站、回收站、版本历史、上传、下载、传输列表），并 SHALL 支持从“文件”App 与照片选择器选择文件上传（只访问用户选中的照片，MUST NOT 申请整个相册权限）、下载后通过分享面板打开或存储到“文件”；离开分享后 MUST 清理沙盒中的明文临时文件。

#### Scenario: 从照片上传
- **WHEN** 用户在 iOS 客户端点“上传 → 照片”并选择两张照片
- **THEN** 两张照片被加密上传，应用没有请求“完全访问相册”的权限

#### Scenario: 下载后存到“文件”
- **WHEN** 用户下载一个 PDF 并在分享面板选择“存储到文件”
- **THEN** 文件内容与云端一致，关闭后 App 沙盒内不再保留该明文文件

#### Scenario: iOS 上传、Web 下载
- **WHEN** 用户在 iOS 客户端上传一个文件
- **THEN** 网页与桌面客户端都能看到该文件并下载得到相同内容

### Requirement: Face ID 解锁
iOS 客户端 SHALL 提供默认关闭的“使用 Face ID 解锁”：开启时主密钥存入仅本机、需生物识别访问的 Keychain 条目；已录入的面容变更后 MUST 要求重新输入密码；退出登录、修改密码或会话失效时 MUST 清除该条目。

#### Scenario: Face ID 解锁
- **WHEN** 开启了 Face ID 解锁的用户重新打开应用并通过 Face ID 验证
- **THEN** 直接进入文件页，无需输入密码

#### Scenario: 面容变更后要求密码
- **WHEN** 用户在系统设置中新增了一个面容后打开应用
- **THEN** 应用要求输入密码解锁

### Requirement: iOS 前台传输
iOS 客户端 SHALL 只在前台传输；切到后台时 SHALL 尽量完成当前分块后暂停，回到前台后 MUST 自动从已确认的分块继续。

#### Scenario: 切到后台再回来
- **WHEN** 上传大文件时用户切到其他 App，几分钟后回来
- **THEN** 上传自动继续，不从头开始

### Requirement: iOS 构建
CI SHALL 在每次提交时构建不签名的 iOS 工程以验证可编译。

#### Scenario: iOS 构建检查
- **WHEN** 提交改动了客户端代码
- **THEN** CI 的 iOS `--no-codesign` 构建通过
