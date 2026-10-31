import 'package:flutter/material.dart';

import 'placeholder_page.dart';

/// 回收站（第 5 组实现恢复与永久删除）。
class TrashPage extends StatelessWidget {
  const TrashPage({super.key});

  @override
  Widget build(BuildContext context) {
    return const PlaceholderPage(
      title: '回收站',
      description: '恢复误删的文件，或确认后永久删除并释放空间。',
      icon: Icons.delete_outline,
    );
  }
}
