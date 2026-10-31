import 'dart:io';

import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';

import 'core/router/app_router.dart';
import 'core/router/destinations.dart';
import 'core/theme/app_theme.dart';
import 'desktop/router/desktop_routes.dart';

/// 当前构建是否启用桌面专有路由（Windows / macOS）。
final bool isDesktopPlatform = Platform.isWindows || Platform.isMacOS;

/// 桌面端侧栏目的地；iOS 等平台过滤掉 `desktopOnly`。
List<AppDestination> destinationsForPlatform({bool? desktop}) {
  final enabled = desktop ?? isDesktopPlatform;
  return enabled
      ? appDestinations
      : appDestinations
            .where((destination) => !destination.desktopOnly)
            .toList(growable: false);
}

/// 生产路由（桌面入口注入备份文件夹页面）。
final GoRouter appRouter = buildAppRouter(
  destinations: destinationsForPlatform(),
  extraShellRoutes: isDesktopPlatform
      ? <RouteBase>[backupFoldersRoute]
      : const <RouteBase>[],
);

/// 应用根组件。
class EncryptedBackupApp extends StatelessWidget {
  const EncryptedBackupApp({this.router, super.key});

  /// 测试可注入自定义路由；默认使用生产路由。
  final GoRouter? router;

  @override
  Widget build(BuildContext context) {
    return MaterialApp.router(
      title: '加密备份',
      debugShowCheckedModeBanner: false,
      theme: AppTheme.light,
      darkTheme: AppTheme.dark,
      themeMode: ThemeMode.system,
      routerConfig: router ?? appRouter,
    );
  }
}
