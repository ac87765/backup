# Proposal

## Why

桌面客户端（`add-native-clients`）解决了电脑上的自动备份。手机上的需求更简单：随时查看、取出云端文件，偶尔把手机上的文件或几张照片传上去。Web 版在手机浏览器里可用但体验一般，也无法使用 Face ID 解锁与系统分享面板。

路线图中 iOS 排在最后：先完成 Windows / macOS（`add-native-clients`），再做微信备份（`add-wechat-backup`），最后做本 change。

## What Changes

- 在 `apps/client` 中启用 iOS 平台，复用 `crates/client-core` 的平台无关部分（认证、文件操作、传输、状态库）与 Flutter 通用页面，不编译桌面专有的备份引擎。
- **只做文件管理**：登录 / 解锁、待审核提示、浏览（列表 / 网格、排序、筛选）、新建文件夹、重命名、移动、删除到回收站、回收站、版本历史、上传、下载、传输列表、退出登录。不做相册自动备份。
- **iOS 专有**：
  - 上传来源：系统文件选择器（“文件”App / iCloud Drive）与照片选择器（只访问用户选中的照片）。
  - 下载后通过分享面板打开、存储到“文件”或分享给其他 App，离开后清理沙盒中的明文临时文件。
  - 可选 Face ID 解锁（“保持解锁”的 iOS 形式）。
  - 底部标签栏布局、列表左滑操作、底部操作菜单。
- **分发**：CI 构建不签名的 iOS 工程验证可编译；TestFlight 需要用户本人的 Apple 开发者账号，作为用户自己完成的步骤。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `native-clients`：新增 iOS 文件操作、Face ID 解锁与 iOS 构建要求（本 change 在 `add-native-clients` 归档后实施）。

## Impact

- `apps/client` 新增 `ios/` 平台工程与 iOS 布局；`client-core` 增加 iOS Keychain（Face ID 访问控制）。
- CI 新增 iOS 构建（macos 运行器，`--no-codesign`）。
- Figma：iOS 页面（登录 / 解锁、文件列表与操作菜单、上传来源、传输、分享、设置），在本 change 开始时绘制。
- 开发机没有 Mac：编译由 CI 保证，真机验收依赖用户的 iPhone。
