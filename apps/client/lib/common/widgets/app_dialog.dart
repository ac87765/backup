import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/theme/design_tokens.dart';
import 'controls.dart';

/// 对话框外壳：tokens.css `.dialog` 的头部 / 主体 / 底部结构。
class TokensDialogShell extends StatelessWidget {
  const TokensDialogShell({
    required this.title,
    required this.body,
    required this.actions,
    this.headerIcon,
    this.headerIconColor,
    this.headerIconBackground,
    this.width = DesktopMetrics.dialogWidth,
    this.onClose,
    super.key,
  });

  final String title;
  final Widget body;
  final List<Widget> actions;
  final IconData? headerIcon;
  final Color? headerIconColor;
  final Color? headerIconBackground;
  final double width;
  final VoidCallback? onClose;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final maxHeight =
        MediaQuery.sizeOf(context).height - DesktopMetrics.dialogMaxHeightInset;
    return Material(
      type: MaterialType.transparency,
      child: Align(
        alignment: Alignment.center,
        child: ConstrainedBox(
          constraints: BoxConstraints(
            maxWidth: width,
            maxHeight: maxHeight < 0 ? 0 : maxHeight,
          ),
          child: Container(
            decoration: BoxDecoration(
              color: tokens.surface,
              borderRadius: BorderRadius.circular(tokens.radiusCard),
              boxShadow: tokens.shadowPop,
            ),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(24, 20, 24, 8),
                  child: Row(
                    children: [
                      if (headerIcon != null) ...[
                        Container(
                          width: 32,
                          height: 32,
                          decoration: BoxDecoration(
                            color: headerIconBackground ?? tokens.canvas,
                            borderRadius: BorderRadius.circular(6),
                          ),
                          child: Icon(
                            headerIcon!,
                            size: 18,
                            color: headerIconColor ?? tokens.ink600,
                          ),
                        ),
                        const SizedBox(width: 12),
                      ],
                      Expanded(
                        child: Text(
                          title,
                          style: TextStyle(
                            color: tokens.ink900,
                            fontSize: tokens.textLg,
                            fontWeight: FontWeight.w600,
                          ),
                        ),
                      ),
                      TokensIconButton(
                        icon: LucideIcons.x,
                        tooltip: '关闭',
                        onPressed:
                            onClose ?? () => Navigator.of(context).maybePop(),
                      ),
                    ],
                  ),
                ),
                Flexible(
                  child: SingleChildScrollView(
                    padding: const EdgeInsets.fromLTRB(24, 12, 24, 20),
                    child: body,
                  ),
                ),
                Container(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 24,
                    vertical: 16,
                  ),
                  decoration: BoxDecoration(
                    border: Border(top: BorderSide(color: tokens.line)),
                  ),
                  child: Row(
                    mainAxisAlignment: MainAxisAlignment.end,
                    children: [
                      for (
                        var index = 0;
                        index < actions.length;
                        index += 1
                      ) ...[
                        if (index > 0) const SizedBox(width: 8),
                        actions[index],
                      ],
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// 带 180ms 缩放淡入的对话框；系统开启“减少动态效果”时立即显示。
Future<T?> showTokensDialog<T>({
  required BuildContext context,
  required WidgetBuilder builder,
}) {
  final tokens = DesktopTokens.of(context);
  final reduceMotion = MediaQuery.disableAnimationsOf(context);
  return showGeneralDialog<T>(
    context: context,
    barrierDismissible: true,
    barrierLabel: '关闭对话框',
    barrierColor: tokens.scrim,
    transitionDuration: reduceMotion ? Duration.zero : tokens.durationBase,
    pageBuilder: (dialogContext, animation, secondaryAnimation) {
      return builder(dialogContext);
    },
    transitionBuilder: (dialogContext, animation, secondaryAnimation, child) {
      if (reduceMotion) {
        return child;
      }
      final curved = CurvedAnimation(parent: animation, curve: tokens.easeOut);
      return FadeTransition(
        opacity: curved,
        child: ScaleTransition(
          scale: Tween<double>(begin: 0.97, end: 1).animate(curved),
          child: child,
        ),
      );
    },
  );
}
