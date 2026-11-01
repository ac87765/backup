# macOS 版接手说明（给在 Mac 上新开会话的 AI）

> 本文件是 macOS 版验收与修复的交接文档。用户只有 Windows 开发机，Windows 版在 Windows 上完成；
> macOS 版由用户之后在自己的 Mac 上新开一个 AI 会话完成。请先完整读完本文件再动手。
> 本文件由审查者维护，每完成一组会更新“当前状态”。

## 0. 分工（2026-10-31 用户确认）

- **macOS 安装包由 CI 生成，不在 Mac 上本地打包。** GitHub Actions 的 macos 运行器构建、ad-hoc 签名并生成 `.dmg`，
  作为 Actions artifact 下载（第 7 组实现；打包脚本为 `apps/client/tool/package_macos.sh`）。
- **Mac 上的会话默认只做验收**：下载 CI 产物 → 安装 → 按第 7 节清单逐项验收 → 记录问题。这一步不需要安装 Xcode、Rust、Flutter。
- **只有验收发现需要改代码的问题时**，才按第 4–5 节在 Mac 上搭建开发环境、修复并补测试；修复后仍以 CI 产出的安装包为准再次验收。
  第 6 节的本地打包步骤只用于排查 CI 打包问题。

## 1. 先读什么

1. `AGENTS.md`、`CLAUDE.md`：项目硬性规则（加密不可变、密钥边界、日志禁止项、检查命令）。必须遵守。
2. `openspec/changes/add-native-clients/`：桌面客户端的 proposal、design（D1–D10）、spec、tasks。
   `tasks.md` 的勾选状态就是真实进度。
3. `design/desktop/`：界面稿（HTML），`?os=mac` 查看 macOS 版式；截图在 `design/desktop/screenshots/`（`*-mac*.png`）。
4. `docs/crypto-format.md`：加密格式（只读，永不修改）。

## 2. 当前状态（2026-10-31）

| 组 | 内容 | 状态 |
|---|---|---|
| 1–3 | `crates/client-core`：认证、钥匙串、文件、传输、状态库、桌面备份引擎 | 已合并 main |
| 4 | `apps/client` Flutter 工程 + `apps/client/rust` 桥接 + CI（`.github/workflows/client.yml` 含 macOS 构建 job） | 已完成 |
| 5 | 通用界面：登录 / 解锁、文件浏览、传输、设置（golden 截图见 `apps/client/test/goldens/`） | 已完成 |
| 6 | 桌面专有：托盘 / 菜单栏、关闭到托盘、单实例、备份文件夹页、拖拽、上传 / 下载文件夹、首次引导、开机启动 | 已完成 |
| 7 | Windows 安装包与 Windows 真机验收；CI 生成 macOS `.dmg`；macOS 验收（本文件的任务） | 进行中（待真机验收） |

macOS 验收应在第 1–7 组的 Windows 部分全部合并、Windows 版验收通过之后再做。开始前先 `git pull`，并以 `tasks.md` 为准核对状态。

### 获取 CI 产物并安装

1. 仓库 Actions 页面 → 工作流 “Client (Flutter / bridge / desktop)” → 选 `main` 分支最近一次成功的运行
   （或 `v*` 标签的发布运行）→ 页面底部 Artifacts 下载 `encrypted-backup-macos`。
   也可用 `gh run download <run-id> -n encrypted-backup-macos`。
   artifact 默认保留 90 天，过期后重新运行工作流即可。
2. 解压得到 `加密备份-<版本>-macos.dmg`，双击打开，把应用拖到“应用程序”，然后推出 dmg。
3. 首次打开：应用未经 Apple 公证，会提示无法验证开发者。在访达中右键应用 →“打开”→ 确认；或执行
   `xattr -dr com.apple.quarantine "/Applications/加密备份.app"` 去掉下载隔离标记。
4. 架构：CI 产物是 **universal**（2026-10-31 的 CI 日志 `lipo -archs` 输出 `x86_64 arm64`），Intel 与 Apple Silicon 都能运行。
   若应用无法启动，用 `lipo -archs "/Applications/加密备份.app/Contents/MacOS/加密备份"` 核对，并记录为问题。

## 3. 至今在 macOS 上从未真机运行过的部分（重点验证）

这些代码只在 GitHub Actions 的 macos 运行器上编译 / 跑过单元测试，没有在真实 Mac 上交互运行过：

- **钥匙串**：`crates/client-core/src/secret_store/macos.rs`（`security-framework`，
  `kSecAttrAccessibleWhenUnlockedThisDeviceOnly`）。验证：登录后会话令牌写入钥匙串；“保持解锁”后重启应用免输密码；
  退出登录 / 改密 / 会话失效后条目被删除（钥匙串访问.app 中确认）。
- **文件监听**：`notify` 在 macOS 用 FSEvents（`crates/client-core/src/desktop/watcher.rs`）。验证新增 / 修改文件约 5 秒后自动备份。
- **文件名规范化**：macOS 文件名是 NFD，上传前统一 NFC（`desktop/filter.rs`）。验证：带中文、带重音字母（如 `é`）的文件，
  Mac 上传后在 Web 端与 Windows 客户端显示同一个文件，不出现重复。
- **路径显示**：界面稿中 macOS 路径显示为 `~/...`，设备名形如“XX 的 MacBook Pro”，托盘文案为“菜单栏”。
- **菜单栏图标与关闭行为**：关闭窗口只隐藏到菜单栏，从菜单栏菜单退出才结束进程（design D7）。
- **开机启动**：Login Item（默认关闭，设置中开启）。
- **窗口外框**：红绿灯按钮位置与 `design/desktop/*.html?os=mac` 一致。
- **未启用 App Sandbox**（design D10）：`apps/client/macos/Runner/*.entitlements` 中 `app-sandbox` 为 false。验证：添加备份文件夹后重启 Mac，备份仍能读取该文件夹；拖拽任意位置的文件可上传。
  更新应用后首次启动若弹出“是否允许访问钥匙串”，选“始终允许”，属预期（ad-hoc 签名每次构建不同）。
- **托盘图标**：菜单栏使用模板 PNG（`apps/client/assets/tray/`），验证深色 / 浅色菜单栏下都清晰；单实例由系统保证。

## 4. 环境准备（Mac，仅在需要修代码时）

- Xcode（App Store）与命令行工具：`xcode-select --install`。
- Rust：按仓库根 `rust-toolchain.toml` 安装（rustup 会自动选版本）；目标：
  `rustup target add aarch64-apple-darwin x86_64-apple-darwin`。
- Flutter **3.47.7**（与 CI、Windows 开发机一致，不要升级）：`flutter config --enable-macos-desktop`。
- `cargo install flutter_rust_bridge_codegen --version 2.13.0 --locked`（必须与 `apps/client/rust/Cargo.toml` 中
  `flutter_rust_bridge` 版本一致）。
- CocoaPods（若 `flutter build macos` 提示需要）：`brew install cocoapods`。
- Docker Desktop（可选）：只有运行 `client-core` 的真实服务端集成测试时需要。

## 5. 构建与检查

```bash
# Rust
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p client-core --features desktop          # 需要 Docker 的测试会启动容器
# Flutter
cd apps/client
flutter pub get
flutter analyze
flutter test
flutter build macos --release
```

产物：`apps/client/build/macos/Build/Products/Release/加密备份.app`（`PRODUCT_NAME` 在 `apps/client/macos/Runner/Configs/AppInfo.xcconfig`）。

补充说明：

- 桥接代码由 `flutter_rust_bridge_codegen generate`（在 `apps/client` 下运行）生成并提交到仓库（`apps/client/rust/src/frb_generated.rs`、
  `apps/client/lib/src/rust/**`）。修改 `apps/client/rust/src/api/**` 后必须重新生成，CI 会用 `git diff` 检查漂移。
- Rust 部分由 cargokit（`apps/client/rust_builder/cargokit`）在 `flutter build` 时编译；macOS 走 `build_pod.sh`。
  本仓库只改过 cargokit 的 Windows 构建目录（`cmake/cargokit.cmake`，规避 MAX_PATH），macOS 路径未改动。
- CI 的 macOS job 只运行不需要 Docker 的 `client-core` 测试目标，并用
  `apps/client/tool/package_macos.sh` 生成 ad-hoc 签名、已校验的 `.dmg`（不再上传未签名的 `.app` zip）。

## 6. 本地打包（仅用于排查 CI 打包问题）

正式安装包由 CI 生成（第 0 节）。需要本地复现 CI 的打包结果时，在 Mac 仓库根目录运行：

```bash
bash apps/client/tool/package_macos.sh
```

脚本会依次执行 `flutter build macos --release`（注入 `APP_VERSION` / `GIT_COMMIT`）、
`codesign --force --deep --sign -` 与 `codesign --verify --deep --strict`、`lipo -archs`，
再用 `hdiutil` 生成只读压缩 dmg（内含应用与 `/Applications` 快捷方式），输出
`apps/client/build/installer/加密备份-<版本>-macos.dmg`。可以用环境变量覆盖版本与提交：

```bash
APP_VERSION=1.2.3 GIT_COMMIT=0123abc bash apps/client/tool/package_macos.sh
```

补充说明：

- 自用分发，不做 Apple 公证（design D10）；dmg 里的应用是 ad-hoc 签名，未启用 App Sandbox。
- 架构：Flutter macOS release 默认只构建当前 Mac 的架构（Apple Silicon → arm64，Intel → x86_64），
  不是 universal；脚本会把 `lipo -archs` 打印到日志。需要 universal 时另行用 xcodebuild 指定
  `ARCHS="arm64 x86_64"`，并在验收记录中注明。
- 首次打开：未公证的应用需在访达中右键“打开”，或在“系统设置 → 隐私与安全性”中允许。
  完整用户步骤见 [`docs/desktop-client.md`](desktop-client.md)。

## 7. 验收（tasks 7.4）

按 [`docs/acceptance/macos.md`](acceptance/macos.md) 逐项执行。该清单与 Windows 清单结构相同，
覆盖：登录 / 待审核 / 解锁、浏览与文件操作、上传下载、文件夹备份全流程（新增 / 修改 / 本地删除不影响云端 /
内容不变不产生新版本 / 排除临时文件）、第 3 节列出的 macOS 专有项（钥匙串、FSEvents、NFD 文件名、
菜单栏模板图标、未启用沙箱、Gatekeeper、架构核对），以及与 Web 和 Windows 客户端互通
（同一文件互相可见、可解密）。

发现问题时记录现象、复现步骤与日志（日志不得包含令牌、密钥、文件内容），修复须遵守 `AGENTS.md`，并补测试。

### 自动更新验收（add-client-releases 4.3）

前提：服务器上已发布比本机版本更高的版本；只有标签构建（`v0.1.0` 这样的版本号）才会提示更新，开发构建不提示。

1. 从 `https://120.79.245.99/download` 下载 .dmg，核对 SHA-256，安装到“应用程序”并按第一次打开的说明放行。
2. 发布新版本后，启动约 10 秒内（或在设置 → 关于点“检查更新”）出现横幅、关于中的新版本与托盘菜单项。
3. 点“下载并安装”：进度正常，校验后弹出重新启动确认；确认后应用退出、被替换并自动重新打开，“关于”显示新版本，登录状态与备份文件夹保留。
4. 确认应用已通过替换启动：Gatekeeper 是否对替换后的应用再次拦截，如拦截，记录现象。
5. 把应用放到不可写的位置（例如只读卷）再更新：应打开 .dmg 并提示拖到“应用程序”，备份继续运行。

Windows 已验证的结果与发布流程见 `deploy/server/README.md` 的“客户端发布”一节；发布签名私钥只在维护者的 Windows 电脑上，不要尝试在 Mac 上发布。

## 8. 注意事项

- 不要修改加密格式、算法参数、AAD / HKDF 字符串（`AGENTS.md`）。
- 不要把任何密钥、令牌、服务器密码写进仓库或日志；服务器部署与账号凭据只由用户提供，不在本文件中。
- 生产服务器的任何写操作都需要用户明确同意。
