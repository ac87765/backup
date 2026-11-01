import 'package:go_router/go_router.dart';

import '../pages/backup_folders_page.dart';
import '../pages/forced_update_page.dart';
import '../pages/onboarding_page.dart';
import '../pages/wechat_page.dart';

/// 桌面专有路由：备份文件夹与首次引导。
///
/// 只在 Windows / macOS 入口注入，core 路由不引用 desktop。
final RouteBase backupFoldersRoute = GoRoute(
  path: '/backup-folders',
  builder: (context, state) => const BackupFoldersPage(),
);

final RouteBase wechatRoute = GoRoute(
  path: '/wechat',
  builder: (context, state) => const WechatBackupPage(),
);

final RouteBase onboardingRoute = GoRoute(
  path: '/onboarding',
  builder: (context, state) => const OnboardingPage(),
);

/// 强制更新阻断页；由认证 / 路由守卫在所有路由之前重定向到这里。
final RouteBase forcedUpdateRoute = GoRoute(
  path: '/forced-update',
  builder: (context, state) => const ForcedUpdatePage(),
);
