import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:window_manager/window_manager.dart';

import '../../core/platform/window_controls.dart';
import '../../core/theme/design_tokens.dart';

/// 桌面窗口：实际窗口只保留 Flutter 内容，视觉标题栏由 `WindowFrame` 绘制。
///
/// - `TitleBarStyle.hidden`：去掉原生标题栏，避免与视觉标题栏重复；
/// - macOS 通过 `windowButtonVisibility: true` 保留系统红绿灯；
/// - 初始按认证窗口 880×560 创建（D8），已登录的主界面由 `WindowModeDriver`
///   在第一帧后切到 1280×800，避免登录页先以桌面尺寸闪一下；
/// - `--background`（开机启动）只创建窗口，不显示，直接留在托盘。
Future<void> configureDesktopWindow({bool startHidden = false}) async {
  await windowManager.ensureInitialized();
  const options = WindowOptions(
    size: Size(DesktopMetrics.authWindowWidth, DesktopMetrics.authWindowHeight),
    center: true,
    title: '加密备份',
    titleBarStyle: TitleBarStyle.hidden,
    windowButtonVisibility: true,
  );
  await windowManager.waitUntilReadyToShow(options, () async {
    // 认证窗口不可调整大小 / 最大化；进入主界面时 applyMode 会恢复。
    await windowManager.setResizable(false);
    await windowManager.setMaximizable(false);
    if (startHidden) {
      await windowManager.hide();
      return;
    }
    await windowManager.show();
    await windowManager.focus();
  });
}

/// 生产窗口控制：全部转发给 `window_manager`。
///
/// 记住上次主窗口尺寸：离开主界面（切到登录 / 引导）时保存，再次进入主界面
/// 时恢复；用户最大化时不覆盖已记住的普通尺寸。
class DesktopWindowControls implements WindowControls {
  DesktopWindowControls({bool startVisible = true})
    : _visible = ValueNotifier<bool>(startVisible);

  final ValueNotifier<bool> _visible;
  WindowMode? _mode;
  Size? _lastMainSize;

  @override
  ValueListenable<bool> get visible => _visible;

  @override
  Future<void> applyMode(WindowMode mode) async {
    if (_mode == mode) {
      return;
    }
    if (_mode == WindowMode.main) {
      await _rememberMainSize();
    }
    _mode = mode;
    switch (mode) {
      case WindowMode.auth:
        await _makeFixed(
          const Size(
            DesktopMetrics.authWindowWidth,
            DesktopMetrics.authWindowHeight,
          ),
        );
      case WindowMode.onboarding:
        await _makeFixed(
          const Size(
            DesktopMetrics.onboardingWindowWidth,
            DesktopMetrics.onboardingWindowHeight,
          ),
        );
      case WindowMode.forced:
        if (await windowManager.isMaximized()) {
          await windowManager.unmaximize();
        }
        await windowManager.setMinimumSize(
          const Size(
            DesktopMetrics.authWindowWidth,
            DesktopMetrics.authWindowHeight,
          ),
        );
        await windowManager.setResizable(true);
        await windowManager.setMaximizable(true);
        await windowManager.setSize(
          const Size(
            DesktopMetrics.authWindowWidth,
            DesktopMetrics.authWindowHeight,
          ),
        );
        await windowManager.center();
      case WindowMode.main:
        await windowManager.setMinimumSize(
          const Size(
            DesktopMetrics.mainWindowMinWidth,
            DesktopMetrics.mainWindowMinHeight,
          ),
        );
        await windowManager.setResizable(true);
        await windowManager.setMaximizable(true);
        await windowManager.setSize(
          _lastMainSize ??
              const Size(
                DesktopMetrics.mainWindowWidth,
                DesktopMetrics.mainWindowHeight,
              ),
        );
        await windowManager.center();
    }
  }

  Future<void> _makeFixed(Size size) async {
    if (await windowManager.isMaximized()) {
      await windowManager.unmaximize();
    }
    await windowManager.setResizable(false);
    await windowManager.setMaximizable(false);
    // 主窗口的最小尺寸 960×600 会挡住 880×560，先放开。
    await windowManager.setMinimumSize(const Size(1, 1));
    await windowManager.setSize(size);
    await windowManager.center();
  }

  Future<void> _rememberMainSize() async {
    if (await windowManager.isMaximized()) {
      return;
    }
    final size = await windowManager.getSize();
    if (size.width > 0 && size.height > 0) {
      _lastMainSize = size;
    }
  }

  @override
  Future<void> startDragging() => windowManager.startDragging();

  @override
  Future<void> minimize() => windowManager.minimize();

  @override
  Future<void> toggleMaximize() async {
    if (await windowManager.isMaximized()) {
      await windowManager.unmaximize();
    } else {
      await windowManager.maximize();
    }
  }

  @override
  Future<void> hideToTray() async {
    await windowManager.hide();
    _visible.value = false;
  }

  @override
  Future<void> showAndFocus() async {
    if (await windowManager.isMinimized()) {
      await windowManager.restore();
    }
    await windowManager.show();
    await windowManager.focus();
    _visible.value = true;
  }

  @override
  Future<void> quit() async {
    await windowManager.setPreventClose(false);
    await windowManager.destroy();
  }
}
