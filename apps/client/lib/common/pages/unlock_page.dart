import 'package:flutter/material.dart';

import 'placeholder_page.dart';

/// 解锁（第 5 组实现；“保持解锁”未开启时启动后显示）。
class UnlockPage extends StatelessWidget {
  const UnlockPage({super.key});

  @override
  Widget build(BuildContext context) {
    return const Scaffold(
      body: PlaceholderPage(
        title: '解锁',
        description: '输入密码解锁本地主密钥；密码只在 Rust 内派生，不经过界面内存。',
        icon: Icons.lock_outline,
      ),
    );
  }
}
