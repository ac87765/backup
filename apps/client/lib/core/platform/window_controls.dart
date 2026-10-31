import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// 窗口模式（design D8）：窗口尺寸与标题栏按钮随界面切换。
///
/// - [auth]：登录 / 解锁 / 待审核 / 启动页，880×560，不可调整大小、不可最大化；
/// - [onboarding]：首次引导，760×720，不可调整大小；
/// - [main]：主界面，1280×800（记住上次主窗口尺寸），可调整，最小 960×600。
enum WindowMode { auth, onboarding, main }

/// 当前窗口模式，标题栏据此决定是否显示最大化按钮。
class WindowModeController extends Notifier<WindowMode> {
  @override
  WindowMode build() => WindowMode.auth;

  void setMode(WindowMode mode) {
    if (state != mode) {
      state = mode;
    }
  }
}

final windowModeProvider = NotifierProvider<WindowModeController, WindowMode>(
  WindowModeController.new,
);

/// 视觉标题栏与真实窗口之间的薄接口。
///
/// 默认实现是空操作：组件测试、golden 与将来的 iOS 构建不会触达
/// `window_manager` 的平台通道。桌面入口在 `main.dart` 里用
/// `DesktopWindowControls` 覆盖它。
abstract class WindowControls {
  Future<void> startDragging();

  Future<void> minimize();

  Future<void> toggleMaximize();

  /// 按界面切换窗口尺寸与能力（D8）。只在模式变化时由
  /// `WindowModeDriver` 调用；实现不负责显示窗口。
  Future<void> applyMode(WindowMode mode);

  /// 窗口是否可见。隐藏到托盘 / `--background` 启动时为 false，
  /// 界面用 `TickerMode` 停止后台动画。
  ValueListenable<bool> get visible;

  /// 关闭到托盘：隐藏窗口，进程与备份继续。
  Future<void> hideToTray();

  Future<void> showAndFocus();

  /// 真正退出：解除关闭拦截并销毁窗口（调用方先停止备份引擎）。
  Future<void> quit();
}

class NoopWindowControls implements WindowControls {
  const NoopWindowControls();

  static final ValueNotifier<bool> _visible = ValueNotifier<bool>(true);

  @override
  ValueListenable<bool> get visible => _visible;

  @override
  Future<void> applyMode(WindowMode mode) async {}

  @override
  Future<void> hideToTray() async {}

  @override
  Future<void> minimize() async {}

  @override
  Future<void> quit() async {}

  @override
  Future<void> showAndFocus() async {}

  @override
  Future<void> startDragging() async {}

  @override
  Future<void> toggleMaximize() async {}
}

final windowControlsProvider = Provider<WindowControls>((ref) {
  return const NoopWindowControls();
});
