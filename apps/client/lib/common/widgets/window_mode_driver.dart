import 'dart:async';

import 'package:flutter/widgets.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../core/platform/window_controls.dart';

/// 按当前路由切换窗口模式（design D8）。
///
/// 监听 GoRouter 的位置而不是每个页面各自调用：页面只负责内容，窗口尺寸
/// 在根组件统一调整，且只有模式变化时才调用一次 [WindowControls.applyMode]。
/// 实现不显示窗口，因此 `--background` 启动和隐藏到托盘时不会被切模式弹出。
class WindowModeDriver extends ConsumerStatefulWidget {
  const WindowModeDriver({
    required this.router,
    required this.child,
    super.key,
  });

  final GoRouter router;
  final Widget child;

  @override
  ConsumerState<WindowModeDriver> createState() => _WindowModeDriverState();
}

class _WindowModeDriverState extends ConsumerState<WindowModeDriver> {
  WindowMode? _applied;

  @override
  void initState() {
    super.initState();
    widget.router.routerDelegate.addListener(_scheduleSync);
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) {
        unawaited(_sync());
      }
    });
  }

  @override
  void didUpdateWidget(WindowModeDriver oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.router != widget.router) {
      oldWidget.router.routerDelegate.removeListener(_scheduleSync);
      widget.router.routerDelegate.addListener(_scheduleSync);
      _applied = null;
      _scheduleSync();
    }
  }

  @override
  void dispose() {
    widget.router.routerDelegate.removeListener(_scheduleSync);
    super.dispose();
  }

  void _scheduleSync() {
    // 路由在 build / redirect 过程中通知；延迟到帧末读取稳定后的位置。
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) {
        unawaited(_sync());
      }
    });
  }

  Future<void> _sync() async {
    final configuration = widget.router.routerDelegate.currentConfiguration;
    // 首帧路由尚未解析完成时不猜模式，等路由通知后的下一帧再切。
    if (configuration.matches.isEmpty) {
      return;
    }
    final mode = _modeFor(configuration.uri);
    if (mode == _applied) {
      return;
    }
    _applied = mode;
    ref.read(windowModeProvider.notifier).setMode(mode);
    try {
      await ref.read(windowControlsProvider).applyMode(mode);
    } catch (_) {
      // 窗口尺寸切换失败不应影响界面可用性；下次模式变化会重试。
    }
  }

  static WindowMode _modeFor(Uri uri) {
    return switch (uri.path) {
      '/onboarding' => WindowMode.onboarding,
      '/splash' || '/login' || '/unlock' || '/pending' => WindowMode.auth,
      _ => WindowMode.main,
    };
  }

  @override
  Widget build(BuildContext context) => widget.child;
}
