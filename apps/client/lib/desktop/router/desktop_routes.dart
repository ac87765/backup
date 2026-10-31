import 'package:go_router/go_router.dart';

import '../pages/backup_folders_page.dart';

/// 桌面专有路由：备份文件夹（第 6 组实现）。
///
/// 只在 Windows / macOS 入口注入，core 路由不引用 desktop。
final RouteBase backupFoldersRoute = GoRoute(
  path: '/backup-folders',
  builder: (context, state) => const BackupFoldersPage(),
);
