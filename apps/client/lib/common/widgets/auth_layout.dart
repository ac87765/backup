import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/theme/design_tokens.dart';

/// 登录 / 解锁 / 待审核共用的 440×600 窗口内容布局。
class AuthLayout extends StatelessWidget {
  const AuthLayout({required this.child, this.trust, super.key});

  final Widget child;

  /// 窗口底部的安全说明（margin-top: auto）。
  final Widget? trust;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Padding(
      padding: const EdgeInsets.fromLTRB(
        DesktopMetrics.authPadding,
        DesktopMetrics.authPadding,
        DesktopMetrics.authPadding,
        DesktopMetrics.authPaddingBottom,
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Expanded(
            child: SingleChildScrollView(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [child],
              ),
            ),
          ),
          if (trust != null) ...[
            SizedBox(height: tokens.space4),
            DefaultTextStyle.merge(
              style: TextStyle(color: tokens.ink500, fontSize: tokens.textXs),
              child: Center(child: trust!),
            ),
          ],
        ],
      ),
    );
  }
}

/// 表单主标题（20px / 600，上 16、下 4）。
class AuthTitle extends StatelessWidget {
  const AuthTitle(this.text, {super.key});

  final String text;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Padding(
      padding: const EdgeInsets.only(top: 16, bottom: 4),
      child: Text(
        text,
        textAlign: TextAlign.center,
        style: TextStyle(
          color: tokens.ink900,
          fontSize: tokens.textXl,
          fontWeight: FontWeight.w600,
        ),
      ),
    );
  }
}

/// 表单副标题（13px / ink-500，居中）。
class AuthSubtitle extends StatelessWidget {
  const AuthSubtitle(this.text, {super.key});

  final String text;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Text(
      text,
      textAlign: TextAlign.center,
      style: TextStyle(
        color: tokens.ink500,
        fontSize: tokens.textSm,
        height: 1.45,
      ),
    );
  }
}

/// 带间距的表单区域（margin-top: 28px，gap: 14px）。
class AuthForm extends StatelessWidget {
  const AuthForm({required this.children, super.key});

  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: 28),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (var index = 0; index < children.length; index += 1) ...[
            if (index > 0) const SizedBox(height: DesktopMetrics.authFormGap),
            children[index],
          ],
        ],
      ),
    );
  }
}

/// 字段标签 + 控件 + 可选错误文案。
class AuthField extends StatelessWidget {
  const AuthField({
    required this.label,
    required this.child,
    this.error,
    super.key,
  });

  final String label;
  final Widget child;
  final String? error;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          label,
          style: TextStyle(
            color: tokens.ink900,
            fontSize: tokens.textSm,
            fontWeight: FontWeight.w600,
          ),
        ),
        const SizedBox(height: 6),
        child,
        if (error != null) ...[
          const SizedBox(height: 6),
          Text(
            error!,
            style: TextStyle(color: tokens.danger700, fontSize: tokens.textXs),
          ),
        ],
      ],
    );
  }
}

/// 底部链接行（12px，居中，间距 16）。
class AuthLinks extends StatelessWidget {
  const AuthLinks({required this.children, super.key});

  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: 4),
      child: Row(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          for (var index = 0; index < children.length; index += 1) ...[
            if (index > 0) const SizedBox(width: 16),
            children[index],
          ],
        ],
      ),
    );
  }
}

/// 认证页底部外链样式（brand-700 + 外链图标）。
class AuthLink extends StatelessWidget {
  const AuthLink({
    required this.label,
    required this.onPressed,
    this.external = false,
    super.key,
  });

  final String label;
  final VoidCallback? onPressed;
  final bool external;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return InkWell(
      onTap: onPressed,
      borderRadius: BorderRadius.circular(4),
      child: Padding(
        padding: const EdgeInsets.symmetric(vertical: 2),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              label,
              style: TextStyle(color: tokens.brand700, fontSize: tokens.textXs),
            ),
            if (external) ...[
              const SizedBox(width: 4),
              Icon(LucideIcons.externalLink, size: 12, color: tokens.brand700),
            ],
          ],
        ),
      ),
    );
  }
}
