import 'package:flutter/material.dart';

import 'placeholder_page.dart';

/// 安全与隐私（第 5 组实现说明页）。
class SecurityPage extends StatelessWidget {
  const SecurityPage({super.key});

  @override
  Widget build(BuildContext context) {
    return const PlaceholderPage(
      title: '安全与隐私',
      description: '端到端加密说明、保持解锁的风险、本地索引与设备信息。',
      icon: Icons.shield_outlined,
    );
  }
}
