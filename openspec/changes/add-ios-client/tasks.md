# Tasks

> 前置条件：`add-native-clients` 与 `add-wechat-backup` 已完成并归档。迭代流程同其他 change：`design/ios/` 界面稿 → 用户确认 → DeepSeek 实现 → 审查者验收。

## 0. 设计稿（审查者）

- [ ] 0.1 `design/ios/`：iOS 登录 / 解锁（含 Face ID）、文件列表与操作菜单、上传来源选择、传输列表、下载后的分享、设置
- [ ] 0.2 用户确认设计稿

## 1. 平台接入

- [ ] 1.1 `apps/client` 启用 iOS 平台；`client-core` 不带 `desktop` feature 编译为 iOS 静态库并通过桥接调用（design D1）
- [ ] 1.2 CI：iOS `--no-codesign` 构建（design D6）

## 2. 界面

- [ ] 2.1 底部标签栏布局、列表左滑操作、底部操作菜单（design D2）
- [ ] 2.2 golden 截图测试（iPhone 尺寸），审查者对照 `design/ios/screenshots/`

## 3. iOS 专有功能

- [ ] 3.1 文件选择器与照片选择器上传，临时副本清理（design D3）
- [ ] 3.2 下载到沙盒 + 分享面板，离开后与启动时清理明文临时文件（design D3）
- [ ] 3.3 Face ID 解锁（Keychain 访问控制，design D4）
- [ ] 3.4 前后台切换时暂停 / 继续传输（design D5）

## 4. 验收

- [ ] 4.1 安装说明（TestFlight 所需条件）
- [ ] 4.2 用户 iPhone 上的验收清单：登录、浏览、上传（文件 / 照片）、下载并分享、Face ID、与 Web / 桌面互通
