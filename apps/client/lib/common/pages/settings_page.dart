import 'package:flutter/material.dart';

import 'placeholder_page.dart';

/// 设置（第 5 组实现账号、保持解锁、关于）。
class SettingsPage extends StatelessWidget {
  const SettingsPage({super.key});

  @override
  Widget build(BuildContext context) {
    return const PlaceholderPage(
      title: '设置',
      description: '账号、保持解锁（默认关闭）、上传限速、关于与版本信息。',
      icon: Icons.settings_outlined,
    );
  }
}
