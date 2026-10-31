import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/theme/design_tokens.dart';
import '../widgets/app_dialog.dart';
import '../widgets/controls.dart';

/// 通用确认对话框（永久删除、清空等）。
Future<bool> showConfirmDialog({
  required BuildContext context,
  required String title,
  required String message,
  String? detail,
  String confirmLabel = '确认',
  bool danger = false,
  IconData icon = LucideIcons.alertTriangle,
  bool iconWarn = true,
}) async {
  final result = await showTokensDialog<bool>(
    context: context,
    builder: (dialogContext) {
      final tokens = DesktopTokens.of(dialogContext);
      return TokensDialogShell(
        title: title,
        width: DesktopMetrics.dialogWidthSmall,
        headerIcon: icon,
        headerIconBackground: iconWarn ? tokens.warning50 : tokens.danger50,
        headerIconColor: iconWarn ? tokens.warning600 : tokens.danger700,
        body: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              message,
              style: TextStyle(
                color: tokens.ink900,
                fontSize: tokens.textSm,
                height: 1.5,
              ),
            ),
            if (detail != null) ...[
              SizedBox(height: tokens.space2),
              Text(
                detail,
                style: TextStyle(
                  color: tokens.ink500,
                  fontSize: tokens.textXs,
                  height: 1.5,
                ),
              ),
            ],
          ],
        ),
        actions: [
          TokensButton(
            label: '取消',
            onPressed: () => Navigator.of(dialogContext).pop(false),
          ),
          TokensButton(
            label: confirmLabel,
            variant: danger
                ? TokensButtonVariant.danger
                : TokensButtonVariant.primary,
            onPressed: () => Navigator.of(dialogContext).pop(true),
          ),
        ],
      );
    },
  );
  return result ?? false;
}
