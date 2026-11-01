# Proposal

## Why

桌面客户端已经可以安装使用，但之后会持续迭代：用户需要一种可靠的方式拿到新版本，而不是每次手动找安装包。同时，新用户需要一个能直接下载客户端的页面——目前安装包只能从 GitHub Actions 的构建产物里取，普通用户找不到。

这是一个端对端加密产品，**更新渠道本身不能成为后门**：如果服务器被入侵，攻击者替换安装包或版本信息，就能绕过整个加密设计。因此更新必须由服务器以外保存的密钥签名，客户端验证签名后才安装。

## What Changes

- **发布流程**：打 `v*` 标签 → CI 构建 Windows 安装包与 macOS .dmg（已有）→ 维护者在开发机上运行发布脚本：取回 CI 产物、计算 SHA-256、生成版本清单并用**离线 Ed25519 私钥签名**、上传到服务器的下载目录。私钥只在维护者的电脑上（及其离线备份），不进入仓库、CI 或服务器。
- **服务器**：新增静态下载目录（Caddy 直接提供 `/downloads/`），存放各版本安装包、`latest.json` 与签名。服务端程序不变。
- **客户端更新**（Windows / macOS）：启动时与每 24 小时检查一次版本清单，用内置公钥验证签名；有新版本时在设置“关于”、托盘菜单和主窗口顶部提示，用户确认后下载、校验大小与 SHA-256、安装并重启。支持“最低支持版本”：低于它的客户端必须更新后才能继续（用于不兼容的协议变化）。
- **下载页面**：网站新增公开页面 `/download`（无需登录）：自动识别系统给出主按钮、另一个平台的下载、版本 / 日期 / 大小 / SHA-256、系统要求、首次打开说明（SmartScreen / Gatekeeper）、更新日志、iOS“即将推出”。登录页、网页版设置和桌面客户端“关于”都链接到它。界面按网页版现有设计体系精心设计（`design/web/download.html`）。

## Capabilities

### New Capabilities

- `client-releases`：签名的版本清单与安装包发布、桌面客户端检查与安装更新、公开下载页面。

### Modified Capabilities

无（`native-clients` 尚在 `add-native-clients` 中，未归档；本 change 只新增能力）。

## Impact

- 新增依赖：`ed25519-dalek`（Rust，验证与签名；理由与 `cargo deny` 见 design）。
- `crates/client-core`：更新检查、清单验签、下载校验（`desktop` feature）。
- `apps/client`：更新提示与“关于”页；Windows 静默安装、macOS 替换应用。
- `web/`：公开路由 `/download`。
- `deploy/`：Caddyfile 模板新增 `/downloads/`；发布脚本 `deploy/release-client.*`；服务器新增目录 `/opt/backup/downloads`（部署时需要用户同意写入服务器）。
- 界面稿：`design/web/download.html`、`design/desktop/update.html`。
