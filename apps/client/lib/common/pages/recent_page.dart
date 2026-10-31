import 'package:flutter/material.dart';

import 'placeholder_page.dart';

/// 最近（第 5 组实现）。
class RecentPage extends StatelessWidget {
  const RecentPage({super.key});

  @override
  Widget build(BuildContext context) {
    return const PlaceholderPage(
      title: '最近',
      description: '按修改时间查看最近变化的文件。',
      icon: Icons.history,
    );
  }
}
