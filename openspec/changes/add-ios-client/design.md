# Design

## Context

- 本 change 建立在 `add-native-clients` 之上：`client-core` 已把平台无关模块与 `desktop` feature 分开，Flutter 工程已把通用页面与桌面专有页面分目录（见该 change 的 design D1/D8）。密钥边界（D2）、传输引擎（D4）、本地状态库（D5）原样适用。
- 产品决策（用户已确认）：iOS 只做文件管理，不做相册自动备份；分发以自用为主。
- 没有 Mac：只能在 GitHub Actions 的 macos 运行器上构建；签名与 TestFlight 需用户本人的 Apple 开发者账号。

## Goals / Non-Goals

**Goals：** 在 iPhone 上可靠地浏览、取出、上传文件；与 Web / 桌面完全互通；遵循 iOS 交互习惯。

**Non-Goals：** 相册自动备份；后台持续传输（只在前台传输）；iPad 专门布局（按 iPhone 布局放大可用即可）；App Store 上架；文件提供程序扩展（在“文件”App 里直接浏览云端）。

## Decisions

### D1 复用方式

- `client-core` 以不带 `desktop` feature 的方式编译为 iOS 静态库，经 flutter_rust_bridge 桥接；桥接 API 与桌面相同，只是不暴露备份引擎接口。
- Flutter 通用页面（登录、解锁、文件列表、回收站、版本、传输、设置）复用，按平台选择布局：iOS 使用下述 D2 布局，组件按 Cupertino 习惯调整。

### D2 布局

- 底部标签栏（文件 / 传输 / 设置）+ 导航栈；列表行左滑出现“删除 / 更多”；“更多”与长按打开底部操作菜单；遵循安全区与动态字体；动效遵循 Web D10 并尊重“减少动态效果”。

### D3 文件进出

- 上传：系统文件选择器（`UIDocumentPicker`，含“文件”App 与 iCloud Drive）与照片选择器（`PHPicker`，只读取用户选中的项，不申请整个相册权限，Info.plist 不声明相册完全访问）。选中项先复制到沙盒临时目录，由 Rust 读取上传，完成后删除副本。
- 下载：Rust 下载到沙盒临时目录（校验全部通过后才原子重命名为可见文件），再通过分享面板打开 / 存储到“文件” / 分享；分享面板关闭或离开页面后删除该明文文件；应用启动时清理残留的临时目录。

### D4 解锁与 Face ID

- 会话令牌存入 Keychain（与 macOS 共用 `security-framework` 实现）。
- 可选“使用 Face ID 解锁”（默认关闭）：主密钥存入 Keychain，`kSecAttrAccessibleWhenUnlockedThisDeviceOnly` + `SecAccessControl(biometryCurrentSet)`；更换已录入的面容后条目失效，需要重新输入密码。退出登录、修改密码、会话失效时清除。

### D5 前后台

- 传输只在前台进行；切到后台时向系统申请短时间完成当前分块，之后暂停，回到前台自动继续（续传记录见 add-native-clients D4/D5）。

### D6 构建与分发

- CI：macos 运行器 `flutter build ios --no-codesign` 验证可编译；golden 截图增加 iPhone 尺寸。
- TestFlight / 真机安装：需要用户的 Apple 开发者账号与证书，由用户在 Mac / Xcode 上完成，或之后配置 CI 签名（需要用户提供证书，作为单独步骤）。

## Risks / Trade-offs

- **没有 Mac / 没有签名** → CI 只能保证可编译；真机验收要等用户安装。
- **只在前台传输** → 大文件上传需要保持 App 在前台；界面提示这一点。
- **沙盒明文临时文件** → 只在分享期间存在，离开即删除，启动时清理残留。
