import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'common/state/auth_controller.dart';
import 'common/widgets/window_mode_driver.dart';
import 'core/platform/window_controls.dart';
import 'core/router/app_router.dart';
import 'core/router/destinations.dart';
import 'core/theme/app_theme.dart';
import 'desktop/router/desktop_routes.dart';
import 'desktop/settings/local_settings.dart';
import 'desktop/state/update_controller.dart';

/// 当前构建是否启用桌面专有路由（Windows / macOS）。
///
/// 做成 provider 而不是顶层常量：`dart:io` 的 Platform 反映运行测试的主机，
/// 组件测试需要与主机无关地指定桌面 / 非桌面（CI 在 Linux 上运行）。
final desktopPlatformProvider = Provider<bool>(
  (ref) => Platform.isWindows || Platform.isMacOS,
);

/// 桌面端侧栏目的地；iOS 等平台过滤掉 `desktopOnly`。
List<AppDestination> destinationsForPlatform({required bool desktop}) {
  return desktop
      ? appDestinations
      : appDestinations
            .where((destination) => !destination.desktopOnly)
            .toList(growable: false);
}

/// 生产路由：认证守卫 + 桌面入口注入备份文件夹 / 首次引导。
final routerProvider = Provider<GoRouter>((ref) {
  final refresh = _AuthRefresh(ref);
  ref.onDispose(refresh.dispose);
  final isDesktopPlatform = ref.watch(desktopPlatformProvider);
  return buildAppRouter(
    destinations: destinationsForPlatform(desktop: isDesktopPlatform),
    extraShellRoutes: isDesktopPlatform
        ? <RouteBase>[backupFoldersRoute, wechatRoute]
        : const <RouteBase>[],
    extraRoutes: isDesktopPlatform
        ? <RouteBase>[forcedUpdateRoute, onboardingRoute]
        : const <RouteBase>[],
    authStage: () => ref.read(authControllerProvider).stage,
    needsOnboarding: isDesktopPlatform
        ? () => ref.read(onboardingRequiredProvider)
        : null,
    forcedUpdate: isDesktopPlatform
        ? () => ref.read(updateControllerProvider).forced
        : null,
    refreshListenable: refresh,
  );
});

class _AuthRefresh extends ChangeNotifier {
  _AuthRefresh(Ref ref) {
    _auth = ref.listen(authControllerProvider, (previous, next) {
      notifyListeners();
    });
    _settings = ref.listen(localSettingsProvider, (previous, next) {
      notifyListeners();
    });
    _update = ref.listen(updateControllerProvider, (previous, next) {
      // 只在“强制更新”切换时重算路由，下载进度等变化不触发导航。
      if ((previous?.forced ?? false) != next.forced) {
        notifyListeners();
      }
    });
  }

  late final ProviderSubscription<AuthState> _auth;
  late final ProviderSubscription<LocalSettings?> _settings;
  late final ProviderSubscription<UpdateState> _update;

  @override
  void dispose() {
    _auth.close();
    _settings.close();
    _update.close();
    super.dispose();
  }
}

/// 应用根组件。
class EncryptedBackupApp extends ConsumerWidget {
  const EncryptedBackupApp({this.router, super.key});

  /// 测试可注入自定义路由；默认使用带认证守卫的生产路由。
  final GoRouter? router;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    // 显式类型：否则 ?? 会把 GoRouter? 作为 ref.watch 的推断类型。
    final GoRouter routerConfig = router ?? ref.watch(routerProvider);
    final controls = ref.watch(windowControlsProvider);
    return ValueListenableBuilder<bool>(
      valueListenable: controls.visible,
      // 窗口隐藏到托盘（或 --background 启动）时静音所有 Ticker：
      // 循环动画不再请求帧，展开窗口后从暂停处继续。
      builder: (context, visible, child) {
        return TickerMode(enabled: visible, child: child!);
      },
      child: WindowModeDriver(
        router: routerConfig,
        child: RepaintBoundary(
          key: const Key('golden-root'),
          child: MaterialApp.router(
            title: '加密备份',
            debugShowCheckedModeBanner: false,
            theme: AppTheme.light,
            darkTheme: AppTheme.dark,
            themeMode: ThemeMode.system,
            routerConfig: routerConfig,
          ),
        ),
      ),
    );
  }
}
