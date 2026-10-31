import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/theme/design_tokens.dart';
import 'auth_brand_panel.dart';
import 'window_frame.dart';

/// 认证窗口：整窗 880×560 双栏（左侧 400 品牌面板 + 右侧表单）。
///
/// 不再在大窗口里画一张居中卡片（D8）。品牌面板是独立组件，iOS 以后可传
/// `showBrand: false` 使用不同布局。
class AuthWindow extends StatelessWidget {
  const AuthWindow({
    required this.child,
    this.title = '加密备份',
    this.showBrand = true,
    super.key,
  });

  final Widget child;
  final String title;
  final bool showBrand;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    // 窗口铺满整个原生窗口；画布色只从 8px 圆角外露出来（与 html 截图一致）。
    return ColoredBox(
      color: tokens.designCanvas,
      child: WindowFrame(
        title: title,
        child: LayoutBuilder(
          builder: (context, constraints) {
            final brand =
                showBrand &&
                constraints.maxWidth >= DesktopMetrics.authBrandWidth + 320;
            return Row(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                if (brand) const AuthBrandPanel(),
                Expanded(child: child),
              ],
            );
          },
        ),
      ),
    );
  }
}

/// 右侧表单区（login.html `.pane`，padding 40px 56px 28px）。
class AuthPane extends StatelessWidget {
  const AuthPane({
    required this.children,
    this.footer,
    this.centered = false,
    this.replayKey,
    super.key,
  });

  final List<Widget> children;

  /// 底部安全说明（`.trust`），有值时用 Spacer 推到窗口底部。
  final Widget? footer;

  /// 没有 footer 的页面（待审核）在垂直方向居中（CSS `justify-content:center`）。
  final bool centered;

  /// 变化时重新播放进入动效（登录 / working / error 等状态切换）。
  final Object? replayKey;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(
        DesktopMetrics.authPanePaddingHorizontal,
        DesktopMetrics.authPanePaddingTop,
        DesktopMetrics.authPanePaddingHorizontal,
        DesktopMetrics.authPanePaddingBottom,
      ),
      child: AuthEnterTransition(
        replayKey: replayKey,
        centered: centered,
        footer: footer,
        children: children,
      ),
    );
  }
}

/// 表单元素依次淡入上移 8px：240ms、间隔 40ms、ease-out。
///
/// “减少动态效果”时直接显示最终画面（controller=1），不创建循环动画。
class AuthEnterTransition extends StatefulWidget {
  const AuthEnterTransition({
    required this.children,
    this.footer,
    this.centered = false,
    this.replayKey,
    super.key,
  });

  final List<Widget> children;
  final Widget? footer;
  final bool centered;
  final Object? replayKey;

  @override
  State<AuthEnterTransition> createState() => _AuthEnterTransitionState();
}

class _AuthEnterTransitionState extends State<AuthEnterTransition>
    with SingleTickerProviderStateMixin {
  static const int _itemMs = 240;
  static const int _staggerMs = 40;

  late final AnimationController _controller;
  bool _initialized = false;
  bool _reduce = false;

  int get _itemCount =>
      widget.children.length + (widget.footer == null ? 0 : 1);

  int get _totalMs =>
      _itemMs + _staggerMs * (_itemCount > 1 ? _itemCount - 1 : 0);

  @override
  void initState() {
    super.initState();
    _controller = AnimationController(
      vsync: this,
      duration: Duration(milliseconds: _totalMs),
    );
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final reduce = MediaQuery.disableAnimationsOf(context);
    if (!_initialized) {
      _initialized = true;
      _reduce = reduce;
      if (reduce) {
        _controller.value = 1;
      } else {
        _controller.forward(from: 0);
      }
      return;
    }
    if (reduce == _reduce) {
      return;
    }
    _reduce = reduce;
    if (reduce) {
      _controller.stop();
      _controller.value = 1;
    } else {
      _controller.forward(from: 0);
    }
  }

  @override
  void didUpdateWidget(AuthEnterTransition oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.replayKey != oldWidget.replayKey) {
      if (_reduce) {
        _controller.value = 1;
      } else {
        _controller.forward(from: 0);
      }
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final items = <Widget>[
      for (var index = 0; index < widget.children.length; index += 1)
        _EnterItem(
          controller: _controller,
          startMs: index * _staggerMs,
          totalMs: _totalMs,
          child: widget.children[index],
        ),
    ];
    final footer = widget.footer;
    if (footer != null) {
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          ...items,
          const Spacer(),
          _EnterItem(
            controller: _controller,
            startMs: widget.children.length * _staggerMs,
            totalMs: _totalMs,
            child: footer,
          ),
        ],
      );
    }
    final content = Column(
      mainAxisSize: widget.centered ? MainAxisSize.min : MainAxisSize.max,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: items,
    );
    return widget.centered ? Center(child: content) : content;
  }
}

class _EnterItem extends StatelessWidget {
  const _EnterItem({
    required this.controller,
    required this.startMs,
    required this.totalMs,
    required this.child,
  });

  final AnimationController controller;
  final int startMs;
  final int totalMs;
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final start = startMs / totalMs;
    final end = ((startMs + 240) / totalMs).clamp(0.0, 1.0);
    return AnimatedBuilder(
      animation: controller,
      builder: (context, child) {
        final t = Interval(
          start,
          end,
          curve: tokens.easeOut,
        ).transform(controller.value);
        return Opacity(
          opacity: t,
          child: Transform.translate(
            offset: Offset(0, 8 * (1 - t)),
            child: child,
          ),
        );
      },
      child: child,
    );
  }
}

/// 表单主标题（20px / 600，左对齐；login.html `.pane h1`）。
class AuthTitle extends StatelessWidget {
  const AuthTitle(this.text, {super.key});

  final String text;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Text(
      text,
      style: TextStyle(
        color: tokens.ink900,
        fontSize: tokens.textXl,
        fontWeight: FontWeight.w600,
        height: 1.45,
      ),
    );
  }
}

/// 表单副标题（13px / ink-500）。
class AuthSubtitle extends StatelessWidget {
  const AuthSubtitle(this.text, {this.height = 1.45, super.key});

  final String text;
  final double height;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Text(
      text,
      style: TextStyle(
        color: tokens.ink500,
        fontSize: tokens.textSm,
        height: height,
      ),
    );
  }
}

/// 带间距的表单区域（margin-top，gap: 14px）。
class AuthForm extends StatelessWidget {
  const AuthForm({required this.children, this.top = 28, super.key});

  final List<Widget> children;
  final double top;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: EdgeInsets.only(top: top),
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

/// 字段标签 + 控件 + 可选错误文案（login.html `.field`）。
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
          Row(
            children: [
              Icon(
                LucideIcons.alertTriangle,
                size: 13,
                color: tokens.danger700,
              ),
              const SizedBox(width: 4),
              Expanded(
                child: Text(
                  error!,
                  style: TextStyle(
                    color: tokens.danger700,
                    fontSize: tokens.textXs,
                  ),
                ),
              ),
            ],
          ),
        ],
      ],
    );
  }
}

/// 底部链接行（12px，左对齐，间距 16）。
class AuthLinks extends StatelessWidget {
  const AuthLinks({required this.children, super.key});

  final List<Widget> children;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: 2),
      child: Row(
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
    );
  }
}

/// 已登录账号卡片（login.html unlock 的 `.who`）。
class AuthAccountCard extends StatelessWidget {
  const AuthAccountCard({
    required this.email,
    required this.initial,
    super.key,
  });

  final String email;
  final String initial;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Container(
      margin: const EdgeInsets.only(top: 20),
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
      decoration: BoxDecoration(
        color: tokens.canvas,
        border: Border.all(color: tokens.line),
        borderRadius: BorderRadius.circular(tokens.radiusControl),
      ),
      child: Row(
        children: [
          Container(
            width: 36,
            height: 36,
            alignment: Alignment.center,
            decoration: BoxDecoration(
              color: tokens.brand100,
              shape: BoxShape.circle,
            ),
            child: Text(
              initial,
              style: TextStyle(
                color: tokens.brand700,
                fontSize: 36 * 0.42,
                fontWeight: FontWeight.w600,
              ),
            ),
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  email,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    color: tokens.ink900,
                    fontSize: tokens.textSm,
                    fontWeight: FontWeight.w500,
                  ),
                ),
                const SizedBox(height: 2),
                Text(
                  '上次在这台电脑登录',
                  style: TextStyle(
                    color: tokens.ink500,
                    fontSize: tokens.textXs,
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

/// 底部安全说明（`.trust`，margin-top:auto + padding-top:24）。
class AuthTrust extends StatelessWidget {
  const AuthTrust({
    required this.text,
    this.icon = LucideIcons.info,
    super.key,
  });

  final String text;
  final IconData icon;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Padding(
      padding: const EdgeInsets.only(top: 24),
      child: Row(
        children: [
          Icon(icon, size: 14, color: tokens.ink500),
          const SizedBox(width: 6),
          Expanded(
            child: Text(
              text,
              style: TextStyle(color: tokens.ink500, fontSize: tokens.textXs),
            ),
          ),
        ],
      ),
    );
  }
}

/// 错误时让密码框横向轻抖一次（300ms；login.html `.shake`）。
class ShakeOnTrigger extends StatefulWidget {
  const ShakeOnTrigger({required this.trigger, required this.child, super.key});

  final bool trigger;
  final Widget child;

  @override
  State<ShakeOnTrigger> createState() => _ShakeOnTriggerState();
}

class _ShakeOnTriggerState extends State<ShakeOnTrigger>
    with SingleTickerProviderStateMixin {
  late final AnimationController _controller = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 300),
  );
  bool _reduce = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _reduce = MediaQuery.disableAnimationsOf(context);
    if (_reduce) {
      _controller.stop();
      _controller.value = 0;
    }
  }

  @override
  void didUpdateWidget(ShakeOnTrigger oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.trigger && !oldWidget.trigger && !_reduce) {
      _controller.forward(from: 0);
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  double _offset(double t) {
    if (t <= 0.2) {
      return -6 * (t / 0.2);
    }
    if (t <= 0.4) {
      return -6 + 11 * ((t - 0.2) / 0.2);
    }
    if (t <= 0.6) {
      return 5 - 8 * ((t - 0.4) / 0.2);
    }
    if (t <= 0.8) {
      return -3 + 5 * ((t - 0.6) / 0.2);
    }
    return 2 - 2 * ((t - 0.8) / 0.2);
  }

  @override
  Widget build(BuildContext context) {
    return AnimatedBuilder(
      animation: _controller,
      builder: (context, child) {
        return Transform.translate(
          offset: Offset(_offset(_controller.value), 0),
          child: child,
        );
      },
      child: widget.child,
    );
  }
}
