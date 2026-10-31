import 'package:flutter/material.dart';

import 'placeholder_page.dart';

/// 全部文件（第 5 组实现列表/网格、操作菜单与上传）。
class FilesPage extends StatelessWidget {
  const FilesPage({super.key});

  @override
  Widget build(BuildContext context) {
    return const PlaceholderPage(
      title: '全部文件',
      description: '浏览云端文件夹、列表/网格切换、筛选与排序，上传、下载、重命名、移动与删除到回收站。',
      icon: Icons.folder_outlined,
    );
  }
}
