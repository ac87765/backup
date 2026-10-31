# macOS 版接手说明（给在 Mac 上新开会话的 AI）

> 本文件是 macOS 构建、打包与验收的交接文档。用户只有 Windows 开发机，Windows 版在 Windows 上完成；
> macOS 版由用户之后在自己的 Mac 上新开一个 AI 会话完成。请先完整读完本文件再动手。
> 本文件由审查者维护，每完成一组会更新“当前状态”。

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
| 5–6 | 通用界面、桌面专有界面（托盘 / 菜单栏、备份文件夹页、开机启动） | 未开始 |
| 7 | Windows 安装包与 Windows 真机验收；**macOS 打包与验收（本文件的任务）** | 未开始 |

macOS 版应在第 1–6 组全部合并、Windows 版验收通过之后再做。开始前先 `git pull`，并以 `tasks.md` 为准核对状态。

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

## 4. 环境准备（Mac）

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
- CI 的 macOS job 只运行不需要 Docker 的 `client-core` 测试目标，并上传未签名的 `.app` zip，可作为对照。

## 6. 打包（tasks 7.1 的 macOS 部分）

自用分发，不做 Apple 公证（design D10）：

1. ad-hoc 签名：`codesign --force --deep --sign - "<应用名>.app"`，然后 `codesign --verify --deep --strict` 检查。
2. 生成 dmg：`hdiutil create -volname "加密备份" -srcfolder "<应用名>.app" -ov -format UDZO 加密备份-<版本>.dmg`
   （版本号取自 git 标签，见 design D10）。若第 7 组在仓库里提供了打包脚本，优先使用脚本。
3. 首次打开：未公证的应用需在访达中右键“打开”，或在“系统设置 → 隐私与安全性”中允许。把实际步骤写进安装说明（tasks 7.2）。
4. 架构：优先构建 Apple Silicon（arm64）；用户的 Mac 若是 Intel，改用 x86_64 或 universal，先问用户机型。

## 7. 验收（tasks 7.4）

按第 7 组写入仓库的 macOS 验收清单逐项执行（届时会在本节给出路径）。至少覆盖：登录 / 待审核 / 解锁、浏览与文件操作、
上传下载、文件夹备份全流程（新增 / 修改 / 本地删除不影响云端 / 内容不变不产生新版本 / 排除临时文件）、
第 3 节列出的 macOS 专有项、与 Web 和 Windows 客户端互通（同一文件互相可见、可解密）。
发现问题时记录现象、复现步骤与日志（日志不得包含令牌、密钥、文件内容），修复须遵守 `AGENTS.md`，并补测试。

## 8. 注意事项

- 不要修改加密格式、算法参数、AAD / HKDF 字符串（`AGENTS.md`）。
- 不要把任何密钥、令牌、服务器密码写进仓库或日志；服务器部署与账号凭据只由用户提供，不在本文件中。
- 生产服务器的任何写操作都需要用户明确同意。
