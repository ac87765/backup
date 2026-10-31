import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:window_manager/window_manager.dart';

import '../../app.dart' show routerProvider;
import '../../common/widgets/app_dialog.dart';
import '../../common/widgets/controls.dart';
import '../../core/platform/window_controls.dart';
import '../../core/providers/app_providers.dart';
import '../../core/theme/design_tokens.dart';
import '../settings/local_settings.dart';
import '../tray/tray_service.dart';

/// 桌面运行时：把窗口 / 托盘的生命周期接到 Riverpod 上。
///
/// 只在 `main.dart` 的桌面入口使用；组件测试直接使用 `ProviderScope`，不会
/// 触达 `window_manager` 或托盘原生命令。
class DesktopRuntime extends ConsumerStatefulWidget {
  const DesktopRuntime({required this.child, super.key});

  final Widget child;

  @override
  ConsumerState<DesktopRuntime> createState() => _DesktopRuntimeState();
}

class _DesktopRuntimeState extends ConsumerState<DesktopRuntime>
    with WindowListener {
  bool _started = false;
  bool _closing = false;
  bool _closeInterception = false;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) => _start());
  }

  Future<void> _start() async {
    if (_started) {
      return;
    }
    _started = true;
    try {
      await ref.read(bootstrapProvider.future);
    } catch (_) {
      // 核心初始化失败时仍允许窗口运行；页面会显示可重试的错误。
    }
    if (!mounted) {
      return;
    }
    var trayReady = false;
    try {
      trayReady = await ref.read(trayServiceProvider).start();
    } catch (_) {
      // 托盘初始化失败时保持系统默认关闭行为。
    }
    if (!mounted) {
      return;
    }
    // 托盘不可用时保持系统默认关闭行为，避免窗口被隐藏后无法找回。
    if (trayReady) {
      await windowManager.setPreventClose(true);
      windowManager.addListener(this);
      _closeInterception = true;
    }
  }

  @override
  void onWindowClose() {
    unawaited(_handleClose());
  }

  Future<void> _handleClose() async {
    if (_closing) {
      return;
    }
    _closing = true;
    try {
      final settings = ref.read(localSettingsProvider);
      if (settings != null && !settings.closeHintShown) {
        await ref.read(localSettingsProvider.notifier).markCloseHintShown();
        await _showCloseHint();
      }
    } finally {
      _closing = false;
    }
    await ref.read(windowControlsProvider).hideToTray();
  }

  Future<void> _showCloseHint() async {
    final context = ref
        .read(routerProvider)
        .routerDelegate
        .navigatorKey
        .currentContext;
    if (context == null || !context.mounted) {
      return;
    }
    final tokens = DesktopTokens.of(context);
    final tray = defaultTargetPlatform == TargetPlatform.macOS
        ? '屏幕顶部的菜单栏'
        : '任务栏右下角的托盘';
    await showTokensDialog<void>(
      context: context,
      builder: (dialogContext) => TokensDialogShell(
        title: '关闭窗口后继续备份',
        width: DesktopMetrics.dialogWidthSmall,
        headerIcon: Icons.cloud_sync_outlined,
        headerIconBackground: tokens.brand50,
        headerIconColor: tokens.brand600,
        onClose: () => Navigator.of(dialogContext).pop(),
        body: Text(
          '加密备份会留在$tray里继续工作。'
          '要完全退出，请在托盘菜单中选择“退出”。',
          style: TextStyle(
            color: tokens.ink600,
            fontSize: tokens.textSm,
            height: 1.5,
          ),
        ),
        actions: [
          TokensButton(
            key: const Key('close-hint-confirm'),
            label: '知道了',
            variant: TokensButtonVariant.primary,
            onPressed: () => Navigator.of(dialogContext).pop(),
          ),
        ],
      ),
    );
  }

  @override
  void dispose() {
    if (_closeInterception) {
      windowManager.removeListener(this);
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => widget.child;
}
