import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

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
/// 真正的窗口拖拽/窗口按钮在桌面专有组（6.1）接平台外框；这里只提供与
/// design/desktop 一致的 32px（macOS 28px）标题栏，让页面布局与截图一致。
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

  /// 标题栏关闭按钮；桌面专有窗口外框（6.1）接入前，设置页用它返回主界面。
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

class _TitleBar extends StatelessWidget {
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
  Widget build(BuildContext context) {
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
    return Container(
      height: height,
      color: tokens.canvas,
      child: Row(
        children: [
          if (isMac)
            Padding(
              padding: EdgeInsets.only(left: tokens.space3),
              child: Row(
                children: [
                  _TrafficLight(color: tokens.trafficRed, onTap: onClose),
                  SizedBox(width: tokens.space2),
                  _TrafficLight(color: tokens.trafficYellow),
                  SizedBox(width: tokens.space2),
                  _TrafficLight(color: tokens.trafficGreen),
                ],
              ),
            )
          else
            Padding(
              padding: EdgeInsets.only(left: tokens.space3),
              child: appTitle,
            ),
          if (isMac) ...[
            const Spacer(),
            Center(child: appTitle),
            const Spacer(),
          ] else
            const Spacer(),
          if (!isMac)
            Row(
              children: [
                _CaptionButton(
                  icon: LucideIcons.minus,
                  size: 14,
                  tokens: tokens,
                ),
                _CaptionButton(
                  icon: LucideIcons.square,
                  size: 12,
                  tokens: tokens,
                ),
                _CaptionButton(
                  icon: LucideIcons.x,
                  size: 14,
                  tokens: tokens,
                  onPressed: onClose,
                ),
              ],
            ),
        ],
      ),
    );
  }
}

class _TrafficLight extends StatelessWidget {
  const _TrafficLight({required this.color, this.onTap});

  final Color color;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return GestureDetector(
      onTap: onTap,
      child: Container(
        width: DesktopMetrics.trafficLightSize,
        height: DesktopMetrics.trafficLightSize,
        decoration: BoxDecoration(
          color: color,
          shape: BoxShape.circle,
          boxShadow: [
            BoxShadow(color: tokens.trafficShadow, spreadRadius: 0.5),
          ],
        ),
      ),
    );
  }
}

class _CaptionButton extends StatelessWidget {
  const _CaptionButton({
    required this.icon,
    required this.size,
    required this.tokens,
    this.onPressed,
  });

  final IconData icon;
  final double size;
  final DesktopTokens tokens;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: DesktopMetrics.captionButtonWidth,
      height: DesktopMetrics.titleBarHeight,
      child: InkWell(
        onTap: onPressed,
        child: Center(
          child: Icon(icon, size: size, color: tokens.ink900),
        ),
      ),
    );
  }
}

/// 独立窗口尺寸的页面容器：在画布中央放一个固定尺寸的窗口。
///
/// 认证页 440×600、设置页 760×600 与设计稿一致；窗口大于设计尺寸时居中显示
/// 卡片，小于设计尺寸时铺满（例如用户缩小窗口）。
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
