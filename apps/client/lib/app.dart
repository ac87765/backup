import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import 'common/state/auth_controller.dart';
import 'core/router/app_router.dart';
import 'core/router/destinations.dart';
import 'core/theme/app_theme.dart';
import 'desktop/router/desktop_routes.dart';
import 'desktop/settings/local_settings.dart';

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
        ? <RouteBase>[backupFoldersRoute]
        : const <RouteBase>[],
    extraRoutes: isDesktopPlatform
        ? <RouteBase>[onboardingRoute]
        : const <RouteBase>[],
    authStage: () => ref.read(authControllerProvider).stage,
    needsOnboarding: isDesktopPlatform
        ? () => ref.read(onboardingRequiredProvider)
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
  }

  late final ProviderSubscription<AuthState> _auth;
  late final ProviderSubscription<LocalSettings?> _settings;

  @override
  void dispose() {
    _auth.close();
    _settings.close();
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
    return RepaintBoundary(
      key: const Key('golden-root'),
      child: MaterialApp.router(
        title: '加密备份',
        debugShowCheckedModeBanner: false,
        theme: AppTheme.light,
        darkTheme: AppTheme.dark,
        themeMode: ThemeMode.system,
        routerConfig: router ?? ref.watch(routerProvider),
      ),
    );
  }
}
