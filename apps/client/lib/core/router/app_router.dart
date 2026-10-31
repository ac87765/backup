import 'package:go_router/go_router.dart';

import '../../common/pages/files_page.dart';
import '../../common/pages/login_page.dart';
import '../../common/pages/recent_page.dart';
import '../../common/pages/security_page.dart';
import '../../common/pages/settings_page.dart';
import '../../common/pages/trash_page.dart';
import '../../common/pages/unlock_page.dart';
import '../../common/widgets/app_shell.dart';
import 'destinations.dart';

/// 构建应用路由。
///
/// 通用页面在这里；桌面专有路由（备份文件夹）由 `lib/app.dart` 通过
/// `extraShellRoutes` 注入，避免 `core/` 依赖 `desktop/`（design D8：iOS 只
/// 引用 common + core）。认证守卫与真实页面在第 5 组接入。
GoRouter buildAppRouter({
  List<AppDestination>? destinations,
  List<RouteBase> extraShellRoutes = const [],
}) {
  // 默认只展示通用目的地；桌面入口显式传入完整列表（含“备份文件夹”）。
  final visibleDestinations =
      destinations ??
      appDestinations
          .where((destination) => !destination.desktopOnly)
          .toList(growable: false);
  return GoRouter(
    initialLocation: '/files',
    routes: [
      GoRoute(path: '/login', builder: (context, state) => const LoginPage()),
      GoRoute(path: '/unlock', builder: (context, state) => const UnlockPage()),
      ShellRoute(
        builder: (context, state, child) {
          return AppShell(
            location: state.uri.path,
            destinations: visibleDestinations,
            child: child,
          );
        },
        routes: [
          GoRoute(
            path: '/files',
            builder: (context, state) => const FilesPage(),
          ),
          GoRoute(
            path: '/recent',
            builder: (context, state) => const RecentPage(),
          ),
          GoRoute(
            path: '/trash',
            builder: (context, state) => const TrashPage(),
          ),
          GoRoute(
            path: '/security',
            builder: (context, state) => const SecurityPage(),
          ),
          GoRoute(
            path: '/settings',
            builder: (context, state) => const SettingsPage(),
          ),
          ...extraShellRoutes,
        ],
      ),
    ],
  );
}
