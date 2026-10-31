import 'package:flutter/material.dart';

import '../../common/pages/placeholder_page.dart';

/// 备份文件夹（第 6 组实现：增删改、排除规则、状态与最近活动、立即备份）。
class BackupFoldersPage extends StatelessWidget {
  const BackupFoldersPage({super.key});

  @override
  Widget build(BuildContext context) {
    return const PlaceholderPage(
      title: '备份文件夹',
      description: '添加本地文件夹并增量备份到云端；查看状态、最近活动、排除规则与上传限速。',
      icon: Icons.cloud_sync_outlined,
    );
  }
}
