# 加密备份 · 桌面客户端

Flutter（界面）+ Rust（核心）实现的 Windows / macOS 桌面客户端。加密、API 调用、
传输与文件夹备份全部在 Rust 侧（`crates/client-core`），本工程只通过
flutter_rust_bridge 拿展示数据；密钥不进入 Dart 内存（design D2）。

## 结构

```
lib/
  core/       桥接封装、Riverpod providers、路由、主题
  common/     通用页面与组件（iOS 之后复用）
  desktop/    桌面专有页面、窗口设置、桌面路由（托盘等后续加入）
  src/rust/   flutter_rust_bridge 生成代码（提交到仓库，不要手改）
rust/         Rust 桥接 crate（crate 名 rust_lib_client，cargokit 依赖该名字）
rust_builder/ cargokit 构建插件
windows/ macos/  平台工程
test/        设计令牌校验、Dart 桥接边界测试、页面冒烟测试
```

## 常用命令（在 `apps/client` 下）

```bash
flutter pub get
dart format --set-exit-if-changed lib/main.dart lib/app.dart lib/core lib/common lib/desktop test
flutter analyze
flutter test
flutter build windows --release   # 或 flutter build macos --release
```

`lib/src/rust/**` 是生成代码：不参与 `dart format` 检查（生成器输出保持原样，
CI 用 `git diff --exit-code` 校验是否最新），也不参与 analyzer（避免生成代码的
lint 噪声）；它仍会被 `flutter test` 与构建编译。

## 重新生成绑定

固定 flutter_rust_bridge 2.13.0：

```bash
cargo install flutter_rust_bridge_codegen --locked --version 2.13.0
flutter_rust_bridge_codegen generate
```

生成后必须提交 `rust/src/frb_generated.rs` 与 `lib/src/rust/**`；CI 会重新生成并
要求 `git diff --exit-code`。配置见 `flutter_rust_bridge.yaml`。

## 设计令牌

颜色/圆角/间距/字号/阴影来自 `design/desktop/tokens.css`，集中在
`lib/core/theme/design_tokens.dart`；`test/core/design_tokens_test.dart` 会读取
CSS 校验关键令牌，防止漂移。tokens.css 目前只有亮色，暗色为补充对照色板，
待设计确认。
