import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/platform/window_controls.dart';
import '../../core/theme/design_tokens.dart';

/// 品牌图标：蓝色圆角方块 + 白色锁（tokens.css `.logo`）。
class BrandLogo extends StatelessWidget {
  const BrandLogo({this.size = DesktopMetrics.iconTileSize, super.key});

  final double size;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Container(
      width: size,
      height: size,
      decoration: BoxDecoration(
        color: tokens.brand600,
        borderRadius: BorderRadius.circular(size * 0.28),
      ),
      child: Icon(LucideIcons.lock, color: tokens.onBrand, size: size * 0.55),
    );
  }
}

/// 窗口内框：视觉标题栏 + 内容。
///
/// 视觉标题栏就是真正的窗口标题栏（`TitleBarStyle.hidden`）：拖动区域、
/// 双击最大化与最小化 / 最大化 / 关闭按钮都接到 [WindowControls]。
/// macOS 保留系统红绿灯，这里只留出它们的位置，标题居中。
class WindowFrame extends StatelessWidget {
  const WindowFrame({
    required this.child,
    this.title = '加密备份',
    this.showTitleBar = true,
    this.onClose,
    super.key,
  });

  final Widget child;
  final String title;
  final bool showTitleBar;

  /// 非空时关闭按钮执行页面自己的返回（设置页）；为空时关闭到托盘。
  final VoidCallback? onClose;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final isMac = defaultTargetPlatform == TargetPlatform.macOS;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: tokens.surface,
        borderRadius: BorderRadius.circular(
          isMac ? DesktopMetrics.windowRadiusMac : DesktopMetrics.windowRadius,
        ),
        boxShadow: tokens.shadowWindow,
      ),
      child: ClipRRect(
        borderRadius: BorderRadius.circular(
          isMac ? DesktopMetrics.windowRadiusMac : DesktopMetrics.windowRadius,
        ),
        child: Material(
          color: tokens.surface,
          child: Column(
            children: [
              if (showTitleBar)
                _TitleBar(
                  title: title,
                  tokens: tokens,
                  isMac: isMac,
                  onClose: onClose,
                ),
              Expanded(child: child),
            ],
          ),
        ),
      ),
    );
  }
}

class _TitleBar extends ConsumerWidget {
  const _TitleBar({
    required this.title,
    required this.tokens,
    required this.isMac,
    this.onClose,
  });

  final String title;
  final DesktopTokens tokens;
  final bool isMac;
  final VoidCallback? onClose;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final controls = ref.read(windowControlsProvider);
    final mode = ref.watch(windowModeProvider);
    // 认证 / 引导窗口不可最大化；强制更新页与主界面可最大化。
    final canMaximize = mode == WindowMode.main || mode == WindowMode.forced;
    final height = isMac
        ? DesktopMetrics.titleBarHeightMac
        : DesktopMetrics.titleBarHeight;
    final appTitle = Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        if (!isMac) ...[
          const BrandLogo(size: 16),
          SizedBox(width: tokens.space2),
        ],
        Text(
          title,
          style: TextStyle(
            color: tokens.ink600,
            fontSize: tokens.textXs,
            fontWeight: isMac ? FontWeight.w600 : FontWeight.w400,
          ),
        ),
      ],
    );
    final content = isMac
        // macOS：系统红绿灯浮在 Flutter 内容之上，左侧留出位置，标题居中。
        ? Stack(
            fit: StackFit.expand,
            children: [
              Center(child: appTitle),
              const Align(
                alignment: Alignment.centerLeft,
                child: SizedBox(width: 80),
              ),
            ],
          )
        : Row(
            children: [
              Padding(
                padding: EdgeInsets.only(left: tokens.space3),
                child: appTitle,
              ),
              const Spacer(),
              Row(
                children: [
                  _CaptionButton(
                    key: const Key('caption-minimize'),
                    icon: LucideIcons.minus,
                    size: 14,
                    tokens: tokens,
                    onPressed: controls.minimize,
                  ),
                  if (canMaximize)
                    _CaptionButton(
                      key: const Key('caption-maximize'),
                      icon: LucideIcons.square,
                      size: 12,
                      tokens: tokens,
                      onPressed: controls.toggleMaximize,
                    ),
                  _CaptionButton(
                    key: const Key('caption-close'),
                    icon: LucideIcons.x,
                    size: 14,
                    tokens: tokens,
                    onPressed: onClose == null
                        ? controls.hideToTray
                        : () async => onClose!(),
                  ),
                ],
              ),
            ],
          );
    return GestureDetector(
      key: const Key('window-titlebar'),
      behavior: HitTestBehavior.opaque,
      onPanStart: (_) => controls.startDragging(),
      onDoubleTap: canMaximize ? controls.toggleMaximize : null,
      child: Container(height: height, color: tokens.canvas, child: content),
    );
  }
}

class _CaptionButton extends StatefulWidget {
  const _CaptionButton({
    required this.icon,
    required this.size,
    required this.tokens,
    required this.onPressed,
    super.key,
  });

  final IconData icon;
  final double size;
  final DesktopTokens tokens;
  final Future<void> Function() onPressed;

  @override
  State<_CaptionButton> createState() => _CaptionButtonState();
}

class _CaptionButtonState extends State<_CaptionButton> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final tokens = widget.tokens;
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: widget.onPressed,
        child: Container(
          width: DesktopMetrics.captionButtonWidth,
          height: DesktopMetrics.titleBarHeight,
          color: _hover ? tokens.canvasStrong : Colors.transparent,
          child: Center(
            child: Icon(widget.icon, size: widget.size, color: tokens.ink900),
          ),
        ),
      ),
    );
  }
}

/// 独立窗口尺寸的页面容器：在画布中央放一个固定尺寸的窗口。
///
/// 设置页 760×600 使用；认证页与首次引导已改为整窗布局（D8），不再用居中
/// 卡片。窗口大于设计尺寸时居中显示卡片，小于设计尺寸时铺满。
class AppWindowScaffold extends StatelessWidget {
  const AppWindowScaffold({
    required this.width,
    required this.height,
    required this.child,
    this.title = '加密备份',
    this.canvas = true,
    this.onClose,
    super.key,
  });

  final double width;
  final double height;
  final Widget child;
  final String title;
  final bool canvas;
  final VoidCallback? onClose;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final content = LayoutBuilder(
      builder: (context, constraints) {
        final w = constraints.maxWidth < width ? constraints.maxWidth : width;
        final h = constraints.maxHeight < height
            ? constraints.maxHeight
            : height;
        return Center(
          child: SizedBox(
            width: w,
            height: h,
            child: WindowFrame(title: title, onClose: onClose, child: child),
          ),
        );
      },
    );
    if (!canvas) {
      return content;
    }
    return ColoredBox(color: tokens.designCanvas, child: content);
  }
}
