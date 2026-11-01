import 'package:flutter/widgets.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

/// 侧栏目的地（design/desktop/shared.js：全部文件、最近、备份文件夹、微信备份、
/// 回收站、安全与隐私）。设置从侧栏账号行的齿轮按钮打开，不作为导航项。
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
  AppDestination(path: '/files', label: '全部文件', icon: LucideIcons.folder),
  AppDestination(path: '/recent', label: '最近', icon: LucideIcons.clock),
  AppDestination(
    path: '/backup-folders',
    label: '备份文件夹',
    icon: LucideIcons.cloudUpload,
    desktopOnly: true,
  ),
  AppDestination(
    path: '/wechat',
    label: '微信备份',
    icon: LucideIcons.messageCircle,
    desktopOnly: true,
  ),
  AppDestination(path: '/trash', label: '回收站', icon: LucideIcons.trash),
  AppDestination(path: '/security', label: '安全与隐私', icon: LucideIcons.shield),
];
