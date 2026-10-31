import 'dart:async';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:window_manager/window_manager.dart';

/// 桌面窗口：初始尺寸按 design/desktop/files.html 的 1280×800，最小 960×600。
///
/// 自定义标题栏（Windows 11 按钮 / macOS 红绿灯）在第 6 组实现；本组先用
/// 平台原生外框。
Future<void> configureDesktopWindow() async {
  if (!Platform.isWindows && !Platform.isMacOS) {
    return;
  }
  await windowManager.ensureInitialized();
  const options = WindowOptions(
    size: Size(1280, 800),
    minimumSize: Size(960, 600),
    center: true,
    title: '加密备份',
    titleBarStyle: TitleBarStyle.normal,
  );
  await windowManager.waitUntilReadyToShow(options, () {
    unawaited(windowManager.setTitle('加密备份'));
    unawaited(windowManager.show());
    unawaited(windowManager.focus());
  });
}
