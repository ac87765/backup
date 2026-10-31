import 'package:flutter_riverpod/flutter_riverpod.dart';

/// 视觉标题栏与真实窗口之间的薄接口。
///
/// 默认实现是空操作：组件测试、golden 与将来的 iOS 构建不会触达
/// `window_manager` 的平台通道。桌面入口在 `main.dart` 里用
/// `DesktopWindowControls` 覆盖它。
abstract class WindowControls {
  Future<void> startDragging();

  Future<void> minimize();

  Future<void> toggleMaximize();

  /// 关闭到托盘：隐藏窗口，进程与备份继续。
  Future<void> hideToTray();

  Future<void> showAndFocus();

  /// 真正退出：解除关闭拦截并销毁窗口（调用方先停止备份引擎）。
  Future<void> quit();
}

class NoopWindowControls implements WindowControls {
  const NoopWindowControls();

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
