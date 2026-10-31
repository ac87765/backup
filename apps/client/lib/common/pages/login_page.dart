import 'package:flutter/material.dart';

import 'placeholder_page.dart';

/// 登录（第 5 组实现；注册、忘记密码跳转网页）。
class LoginPage extends StatelessWidget {
  const LoginPage({super.key});

  @override
  Widget build(BuildContext context) {
    return const Scaffold(
      body: PlaceholderPage(
        title: '登录',
        description: '邮箱与密码登录；注册、忘记密码与恢复密钥重置在网页完成。',
        icon: Icons.login,
      ),
    );
  }
}
