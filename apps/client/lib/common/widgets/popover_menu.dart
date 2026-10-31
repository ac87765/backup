import 'package:flutter/material.dart';

import '../../core/theme/design_tokens.dart';

class TokensMenuItem {
  const TokensMenuItem({
    required this.label,
    required this.icon,
    required this.onSelected,
    this.danger = false,
    this.separatorBefore = false,
    this.highlighted = false,
  });

  final String label;
  final IconData icon;
  final VoidCallback onSelected;
  final bool danger;
  final bool separatorBefore;
  final bool highlighted;
}

/// 行操作菜单（tokens.css `.popover.menu`）：220px 宽、32px 行高。
Future<void> showTokensMenu({
  required BuildContext context,
  required Rect anchor,
  required List<TokensMenuItem> items,
}) {
  final tokens = DesktopTokens.of(context);
  final menuHeight =
      12 +
      items.length * DesktopMetrics.menuItemHeight +
      items.where((item) => item.separatorBefore).length * 13;
  return showGeneralDialog<void>(
    context: context,
    barrierDismissible: true,
    barrierLabel: '关闭菜单',
    barrierColor: Colors.transparent,
    transitionDuration: Duration.zero,
    pageBuilder: (dialogContext, animation, secondaryAnimation) {
      final screen = MediaQuery.sizeOf(dialogContext);
      var left = anchor.right - DesktopMetrics.menuWidth;
      if (left < 8) {
        left = 8;
      }
      if (left + DesktopMetrics.menuWidth > screen.width - 8) {
        left = screen.width - DesktopMetrics.menuWidth - 8;
      }
      var top = anchor.bottom + 2;
      if (top + menuHeight > screen.height - 8) {
        top = anchor.top - menuHeight - 2;
      }
      if (top < 8) {
        top = 8;
      }
      return Stack(
        children: [
          Positioned.fill(
            child: GestureDetector(
              behavior: HitTestBehavior.opaque,
              onTap: () => Navigator.of(dialogContext).pop(),
            ),
          ),
          Positioned(
            left: left,
            top: top,
            width: DesktopMetrics.menuWidth,
            child: Container(
              padding: const EdgeInsets.all(6),
              decoration: BoxDecoration(
                color: tokens.surface,
                border: Border.all(color: tokens.line),
                borderRadius: BorderRadius.circular(tokens.radiusCard),
                boxShadow: tokens.shadowPop,
              ),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  for (final item in items) ...[
                    if (item.separatorBefore)
                      Container(
                        height: 1,
                        margin: const EdgeInsets.symmetric(vertical: 6),
                        color: tokens.line,
                      ),
                    _MenuRow(
                      item: item,
                      tokens: tokens,
                      onTap: () {
                        Navigator.of(dialogContext).pop();
                        item.onSelected();
                      },
                    ),
                  ],
                ],
              ),
            ),
          ),
        ],
      );
    },
  );
}

class _MenuRow extends StatefulWidget {
  const _MenuRow({
    required this.item,
    required this.tokens,
    required this.onTap,
  });

  final TokensMenuItem item;
  final DesktopTokens tokens;
  final VoidCallback onTap;

  @override
  State<_MenuRow> createState() => _MenuRowState();
}

class _MenuRowState extends State<_MenuRow> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final tokens = widget.tokens;
    final foreground = widget.item.danger ? tokens.danger700 : tokens.ink900;
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        key: ValueKey<String>('menu-item-${widget.item.label}'),
        behavior: HitTestBehavior.opaque,
        onTap: widget.onTap,
        child: Container(
          height: DesktopMetrics.menuItemHeight,
          padding: const EdgeInsets.symmetric(horizontal: 10),
          decoration: BoxDecoration(
            color: _hover || widget.item.highlighted
                ? tokens.canvas
                : Colors.transparent,
            borderRadius: BorderRadius.circular(6),
          ),
          child: Row(
            children: [
              Icon(widget.item.icon, size: 16, color: foreground),
              const SizedBox(width: 10),
              Text(
                widget.item.label,
                style: TextStyle(color: foreground, fontSize: tokens.textSm),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// 当前行“更多”按钮的锚点；返回菜单需要的屏幕坐标矩形。
Rect anchorOf(BuildContext context) {
  final box = context.findRenderObject();
  if (box is! RenderBox || !box.hasSize) {
    return const Rect.fromLTWH(0, 0, 0, 0);
  }
  return box.localToGlobal(Offset.zero) & box.size;
}
