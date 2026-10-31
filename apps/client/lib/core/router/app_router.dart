import 'package:flutter/foundation.dart';
import 'package:go_router/go_router.dart';

import '../../common/pages/files_page.dart';
import '../../common/pages/login_page.dart';
import '../../common/pages/pending_page.dart';
import '../../common/pages/recent_page.dart';
import '../../common/pages/security_page.dart';
import '../../common/pages/settings_page.dart';
import '../../common/pages/splash_page.dart';
import '../../common/pages/trash_page.dart';
import '../../common/pages/unlock_page.dart';
import '../../common/state/auth_controller.dart';
import '../../common/widgets/app_shell.dart';
import 'destinations.dart';

/// 构建应用路由。
///
/// [authStage] 提供认证阶段用于守卫；为 null 时不启用守卫（组件测试可直接
/// 打开某个页面）。[refreshListenable] 在认证状态变化时触发重新求值。
GoRouter buildAppRouter({
  List<AppDestination>? destinations,
  List<RouteBase> extraShellRoutes = const <RouteBase>[],
  AuthStage Function()? authStage,
  Listenable? refreshListenable,
}) {
  final visibleDestinations =
      destinations ??
      appDestinations
          .where((destination) => !destination.desktopOnly)
          .toList(growable: false);
  return GoRouter(
    initialLocation: '/splash',
    refreshListenable: refreshListenable,
    redirect: (context, state) {
      final stage = authStage?.call();
      if (stage == null) {
        return null;
      }
      final path = state.uri.path;
      switch (stage) {
        case AuthStage.initializing:
        case AuthStage.failure:
          return path == '/splash' ? null : '/splash';
        case AuthStage.login:
          return path == '/login' ? null : '/login';
        case AuthStage.unlock:
          return path == '/unlock' ? null : '/unlock';
        case AuthStage.pending:
          return path == '/pending' ? null : '/pending';
        case AuthStage.ready:
          if (path == '/splash' ||
              path == '/login' ||
              path == '/unlock' ||
              path == '/pending' ||
              path == '/') {
            return '/files';
          }
          return null;
      }
    },
    routes: <RouteBase>[
      GoRoute(path: '/splash', builder: (context, state) => const SplashPage()),
      GoRoute(path: '/login', builder: (context, state) => const LoginPage()),
      GoRoute(path: '/unlock', builder: (context, state) => const UnlockPage()),
      GoRoute(
        path: '/pending',
        builder: (context, state) => const PendingPage(),
      ),
      GoRoute(
        path: '/settings',
        builder: (context, state) => const SettingsPage(),
      ),
      ShellRoute(
        builder: (context, state, child) {
          return AppShell(
            location: state.uri.path,
            destinations: visibleDestinations,
            child: child,
          );
        },
        routes: <RouteBase>[
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
          ...extraShellRoutes,
        ],
      ),
    ],
  );
}
