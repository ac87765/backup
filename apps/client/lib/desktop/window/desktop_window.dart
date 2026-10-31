import 'package:flutter/material.dart';
import 'package:window_manager/window_manager.dart';

import '../../core/platform/window_controls.dart';

/// 桌面窗口：实际窗口只保留 Flutter 内容，视觉标题栏由 `WindowFrame` 绘制。
///
/// - `TitleBarStyle.hidden`：去掉原生标题栏，避免与视觉标题栏重复；
/// - macOS 通过 `windowButtonVisibility: true` 保留系统红绿灯；
/// - `--background`（开机启动）只创建窗口，不显示，直接留在托盘。
Future<void> configureDesktopWindow({bool startHidden = false}) async {
  await windowManager.ensureInitialized();
  const options = WindowOptions(
    size: Size(1280, 800),
    minimumSize: Size(960, 600),
    center: true,
    title: '加密备份',
    titleBarStyle: TitleBarStyle.hidden,
    windowButtonVisibility: true,
  );
  await windowManager.waitUntilReadyToShow(options, () async {
    if (startHidden) {
      await windowManager.hide();
      return;
    }
    await windowManager.show();
    await windowManager.focus();
  });
}

/// 生产窗口控制：全部转发给 `window_manager`。
class DesktopWindowControls implements WindowControls {
  const DesktopWindowControls();

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
  Future<void> hideToTray() => windowManager.hide();

  @override
  Future<void> showAndFocus() async {
    if (await windowManager.isMinimized()) {
      await windowManager.restore();
    }
    await windowManager.show();
    await windowManager.focus();
  }

  @override
  Future<void> quit() async {
    await windowManager.setPreventClose(false);
    await windowManager.destroy();
  }
}
