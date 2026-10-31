import 'package:flutter/material.dart';

/// 侧栏目的地（design/desktop/files.html：全部文件、最近、回收站、备份文件夹、
/// 安全与隐私、设置）。
class AppDestination {
  const AppDestination({
    required this.path,
    required this.label,
    required this.icon,
    this.desktopOnly = false,
  });

  final String path;
  final String label;
  final IconData icon;

  /// 桌面专有（iOS 构建过滤掉，见 `lib/app.dart`）。
  final bool desktopOnly;
}

const appDestinations = <AppDestination>[
  AppDestination(path: '/files', label: '全部文件', icon: Icons.folder_outlined),
  AppDestination(path: '/recent', label: '最近', icon: Icons.history),
  AppDestination(path: '/trash', label: '回收站', icon: Icons.delete_outline),
  AppDestination(
    path: '/backup-folders',
    label: '备份文件夹',
    icon: Icons.cloud_sync_outlined,
    desktopOnly: true,
  ),
  AppDestination(
    path: '/security',
    label: '安全与隐私',
    icon: Icons.shield_outlined,
  ),
  AppDestination(path: '/settings', label: '设置', icon: Icons.settings_outlined),
];
