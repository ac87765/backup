import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/platform/window_controls.dart';
import '../../core/providers/app_providers.dart';

typedef TrayHider = void Function();

/// 完整退出流程：隐藏托盘与窗口 → 停止备份引擎 → 退出进程。
///
/// 托盘“退出”与“安装更新后立即重新启动”共用同一个实现；测试用
/// `appQuitProvider` 注入假回调，避免触达平台通道。
abstract class AppQuit {
  /// 托盘服务启动后注册：退出时先隐藏托盘图标，避免窗口消失后图标还留着。
  void registerTrayHider(TrayHider hider);

  Future<void> quit();
}

class DesktopAppQuit implements AppQuit {
  DesktopAppQuit(this._ref);

  final Ref _ref;
  TrayHider? _hideTray;

  @override
  void registerTrayHider(TrayHider hider) => _hideTray = hider;

  @override
  Future<void> quit() async {
    // 先让用户立刻看到“已退出”：隐藏托盘图标与窗口，再在后台收尾。
    _hideTray?.call();
    final controls = _ref.read(windowControlsProvider);
    try {
      await controls.hideToTray();
    } catch (_) {
      // 隐藏失败不影响退出。
    }
    try {
      // 最多等 3 秒：引擎停不下来也必须能退出。
      await _ref
          .read(nativeBridgeProvider)
          .backupStop()
          .timeout(const Duration(seconds: 3));
    } catch (_) {
      // 退出前尽量停止引擎；失败也要允许退出。
    }
    await controls.quit();
  }
}

final appQuitProvider = Provider<AppQuit>((ref) => DesktopAppQuit(ref));
