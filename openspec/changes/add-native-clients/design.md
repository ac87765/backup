# Design

## Context

- 服务端与加密格式已稳定：Argon2id → HKDF 派生 `pwd_wrap` / `auth_key`；XChaCha20-Poly1305 4 MiB 分块；名称令牌 `HMAC(name_key, parent ‖ NFC(lower(NFC(name))))`；版本元数据可选 `content_sha256`（见已归档的 bootstrap 与 add-folder-backup 设计）。`crypto-core` 是唯一实现，Web 通过 WASM 使用。
- Web 客户端的边界规则：密钥只在 Worker 内存中，主线程只拿展示数据；本设计把同一规则搬到“Rust 核心 / Flutter 界面”之间。
- 产品决策（用户已确认）：桌面端做“文件夹备份”（单向、增量），分发以自用为主。路线图：本 change（Windows + macOS）→ `add-wechat-backup` → `add-ios-client`（iOS 只做文件管理，最后做）。
- 开发环境：Windows 开发机有 Flutter 3.47.7 与 Rust；没有 Mac，macOS 只能在 GitHub Actions 的 macos 运行器上构建，真机运行验收依赖用户设备。

## Goals / Non-Goals

**Goals：** Windows 与 macOS 共用一份 Rust 核心与一份 Flutter 界面代码；与 Web 完全互通（同一账号、同一数据、同一加密格式）；可靠的后台增量备份；界面延续 Dropbox 风格与 `design/desktop/` 界面稿；核心与界面结构为之后的 iOS 客户端（`add-ios-client`）直接复用做好准备。

**Non-Goals：** iOS（单独的 change）；双向同步；Android / Linux；应用商店正式上架；离线文件缓存；在客户端注册账号或用恢复密钥重置密码（跳转网页完成）。

## Decisions

### D1 分层

```
Flutter（apps/client/lib）      只有界面与状态；拿到的都是展示数据（名称、大小、进度）
   │  flutter_rust_bridge v2（异步调用 + Stream 推送进度事件）
Rust 桥接（apps/client/rust）    薄层：把 client-core 的 API 暴露给 Dart，做类型转换
   │
crates/client-core              API 客户端、认证、密钥、传输、扫描与增量、备份调度、本地状态库
   │
crates/crypto-core              现有加密实现（同一测试向量）
```

- `client-core` 是普通 Rust 库，不依赖 Flutter，可单独测试，将来也可用于命令行工具。
- 为 iOS 预留：认证、文件操作、传输、状态库放在与平台无关的模块；桌面专有部分（文件夹备份引擎、文件监听、开机启动）放在 `desktop` feature 下，iOS 构建时不编译。Flutter 侧同样把桌面专有页面与通用页面分目录。
- 网络：`reqwest` + `rustls`（与服务端一致使用 ring 提供者）；会话 Cookie 由 Rust 管理；所有写请求带与 Web 相同的自定义 CSRF 头；`User-Agent` 形如 `EncryptedBackup/<version> (Windows 11; desktop)`。

### D2 密钥边界（硬性）

- 主密钥、节点密钥、内容密钥、`auth_key`、恢复密钥都只存在于 Rust 内存，类型实现 `Zeroize`/`ZeroizeOnDrop`，不实现 `Debug`/`Serialize`（或脱敏）。
- 桥接 API 的任何返回值都不得包含密钥字节或可派生密钥的材料；Dart 侧只收到名称、大小、时间、进度、错误码。用测试枚举全部桥接函数的返回类型，断言不含字节数组字段。
- 下载的明文内容由 Rust 直接写入用户选择的本地路径，不经过 Dart 内存；上传由 Rust 直接读取本地路径。
- 解密失败不得写出部分明文：先写入同目录临时文件，全部校验通过后原子重命名。

### D3 “保持解锁”与凭据存储

- 会话令牌：始终存入系统钥匙串（关掉应用后不用重新输入邮箱）。
- 主密钥：默认**不**持久化——每次启动应用需要输入密码解锁，此时备份暂停并在托盘提示“需要解锁”。
- 可选“在这台电脑上保持解锁”（设置中开启，默认关闭；开启时说明：任何能登录这台电脑系统账号的人都可能读取你的备份）：
  - macOS：存入 Keychain，`kSecAttrAccessibleWhenUnlockedThisDeviceOnly`（不随 iCloud 同步、不进备份）。用 Rust `security-framework` crate 访问，不经过 Dart（iOS 之后复用同一实现）。
  - Windows：DPAPI（当前用户范围）加密后存入凭据管理器（Rust `keyring` crate 的 Windows 原生后端）。
- 退出登录、修改密码或服务端返回会话失效时，清除钥匙串中的会话与主密钥。

### D4 传输引擎（与 Web 行为一致）

- 4 MiB 分块、批量预签名 / 确认（每批最多 32 块）、提交；文件级并发 3；下载逐块校验后解密。
- 429：按 `Retry-After` 退避（缺省 5 秒 + 抖动，最多 8 次），不计为失败（folder-transfer 规格）。
- 断点续传：上传会话与已确认分块记录在本地状态库；应用重启后继续未完成的上传（会话未过期时）。
- 暂停 / 继续 / 取消 / 重试；取消时调用 abort 释放配额预留。
- 读取 Windows 上被其他程序打开的文件使用共享读；文件在上传过程中被修改（大小或修改时间变化）时放弃本次上传，稍后重试。
- 支持超过 260 字符的 Windows 路径（`\\?\` 前缀）。

### D5 本地状态库

- SQLite（`rusqlite`，bundled），位于应用数据目录；只存元数据，不存任何密钥或文件内容：备份文件夹配置、文件索引（本地相对路径、大小、修改时间、云端节点 ID、已备份的 `content_sha256`）、传输队列与续传记录、最近活动。
- 本地路径属于用户自己设备上的信息，以明文保存在本机（与系统文件索引同等级别），在“安全与隐私”说明中写明；服务端看不到这些路径（云端名称仍是密文）。
- 数据库架构有版本号与迁移；损坏时可重建（重新扫描即可恢复索引，不丢云端数据）。

### D6 文件夹备份引擎

- 配置：备份文件夹列表，每项 = 本地路径 + 云端目标文件夹（默认 `全部文件/电脑备份/<设备名>/<本地文件夹名>`，可修改）+ 排除规则。
- 触发：文件系统监听（`notify` crate，5 秒防抖合并）+ 启动时全量核对 + 每 6 小时全量核对；手动“立即备份”。
- 增量规则：本地索引中大小与修改时间均未变 → 直接跳过（不读文件）；否则计算 SHA-256，与云端当前版本元数据中的 `content_sha256` 比较：相同则只更新索引，不同则上传新版本；云端不存在则新建。目录逐层与云端合并（同名合并，与 folder-transfer 相同），`name_conflict` 竞态时重新列出复用。
- 永不删除云端内容：本地删除或重命名的文件在云端保留（重命名表现为新增一个文件）；界面说明这一点。
- 排除：默认排除系统文件（`.DS_Store`、`Thumbs.db`、`desktop.ini`、`._*`、`~$*`、`.~lock.*#`）、临时文件（`*.tmp`、`*.part`、`*.crdownload`）、不跟随符号链接 / 快捷方式；用户可加 glob 规则与排除子文件夹。
- 名称规范化：macOS 文件名是 NFD，上传前统一转为 NFC（与名称令牌规则一致），避免同名文件在云端重复；Windows 大小写不敏感，按名称令牌规则（小写）比较。
- 资源：扫描与哈希在后台线程，限制并发与内存；可设置上传限速；电脑睡眠 / 断网时暂停，恢复后继续。
- 引擎对外提供通用的“把一个本地目录按规则增量备份到云端目录”接口，`add-wechat-backup` 在此之上增加微信专用的来源发现与分类，不另写一套传输逻辑。

### D7 后台运行与托盘

- 关闭主窗口只隐藏到托盘（Windows）/ 菜单栏（macOS），从托盘菜单退出才结束进程；托盘弹窗按 `design/desktop/tray.html`：备份状态、正在上传、最近活动、暂停 / 恢复、打开应用、打开网页版、设置。
- 开机自动启动（默认关闭，设置中开启）：Windows 注册表 Run 项 / macOS Login Item（`launch_at_startup`），只启动程序，不涉及密钥（未“保持解锁”时托盘提示需要解锁）。
- 单实例：重复启动时激活已有窗口。
- 第一版托盘实现（2026-10-31 审查者决定）：托盘图标（四种状态叠加，见 tray.html 注释）+ 系统原生菜单（第一行为状态文字：已全部备份 / 正在备份 n/m / 已暂停 / 需要解锁；然后 暂停或恢复备份、打开应用、打开网页版、设置、退出）；左键点击图标打开主窗口（需要解锁时打开解锁页）。`tray.html` 的富弹窗需要第二个 Flutter 窗口与第二个桥接引擎，风险较高，列为后续改进，不在本 change 范围内。

### D8 Flutter 界面结构

- 状态管理 Riverpod；路由 go_router；设计令牌与 Web D2 一致（颜色、圆角、间距、字号），按平台使用系统字体（Windows：Segoe UI Variable + 微软雅黑；macOS：SF Pro + 苹方）。
- 布局按 `design/desktop/` 界面稿：左侧导航（全部文件、最近、回收站、备份文件夹、安全与隐私）+ 顶部工具栏 + 文件列表 + 状态栏；窗口外框遵循平台（Windows 11 标题栏按钮 / macOS 红绿灯，`window_manager`）。
- 窗口尺寸随界面切换（同一个原生窗口，用 `window_manager` 改尺寸并居中，而不是在大窗口里画一张居中卡片）：登录 / 解锁 / 待审核 880×560（不可调整大小、不可最大化，`login.html`）；首次引导 760×720（`onboarding.html`）；主界面 1280×800（可调整，最小 960×600）。切换时窗口尺寸直接设置、内容用设计稿规定的进入动效，避免窗口动画与内容动画叠加。
- 动效遵循 Web D10（轻、快、尊重系统“减少动态效果”）。

### D9 跨端一致性与测试

- `client-core` 集成测试复用服务端测试工具（testcontainers 启动 PostgreSQL + MinIO + 服务端），覆盖登录、浏览、上传下载、版本、回收站、文件夹备份增量、429 退避、续传；并验证 Web 上传的文件能被客户端解密、客户端上传的文件能被 Web 解密（共用测试向量 + 端到端互通测试）。
- Flutter：组件测试 + 关键页面 golden 截图（桌面尺寸），审查者对照 `design/desktop/screenshots/` 验收。
- 桥接边界测试（D2）。

### D10 构建与分发

- 版本号取自 git 标签；CI：`flutter analyze`、`flutter test`、`cargo test -p client-core`；Windows 构建（windows 运行器，Inno Setup 生成安装包）；macOS 构建（macos 运行器，ad-hoc 签名的 .app 打包为 .dmg）。产物作为 Actions artifact 下载。macOS 安装包只由 CI 生成：用户在 Mac 上下载安装并验收，只有需要修代码时才在 Mac 上搭开发环境（交接说明见 `docs/macos-handoff.md`）。
- 未签名应用首次打开：Windows SmartScreen 选“仍要运行”；macOS 右键“打开”。在 README 写清楚。
- macOS 不启用 App Sandbox（2026-10-31 审查者决定）：备份文件夹需要跨重启的持续读取权限，沙箱下要为每个文件夹维护 security-scoped bookmark；本应用不经 App Store 分发（与 Dropbox 等备份客户端相同），关闭沙箱即可。副作用：ad-hoc 签名每次构建都不同，更新后首次读取钥匙串条目时系统可能询问是否允许，属预期。

## Risks / Trade-offs

- **没有 Mac** → macOS 的编译由 CI 保证，交互验收依赖用户设备；功能先在 Windows 完整验收，macOS 差异集中在托盘、钥匙串、路径规范化，单独列测试。
- **“保持解锁”降低安全性** → 默认关闭、说明风险、绑定本机系统账号；不开启时备份在重启后暂停直至解锁。
- **本地索引含明文路径** → 只在本机，说明清楚；不上传。
- **文件监听漏事件**（休眠、网络盘）→ 定时全量核对兜底。
- **大目录首次扫描慢** → 后台进行、显示进度；只对变化文件计算哈希。
- **flutter_rust_bridge 版本升级成本** → 固定版本，桥接层保持薄。

## Migration Plan

新增客户端，不影响 Web 与服务端。先发布 Windows 版自用验收，再发布 macOS 构建。
