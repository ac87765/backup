import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../common/widgets/app_dialog.dart';
import '../../common/widgets/controls.dart';
import '../../core/platform/external_links.dart';
import '../../core/providers/app_providers.dart';
import '../../core/theme/design_tokens.dart';
import '../state/update_controller.dart';

/// 打开下载页面（`<serverUrl>/download`）。使用现有的外部链接 provider，
/// 测试里只记录 URI，不触达平台通道。
Future<void> openUpdateDownloadPage(BuildContext context, WidgetRef ref) async {
  try {
    final config = await ref.read(appConfigProvider.future);
    if (!context.mounted) {
      return;
    }
    await ref.read(externalLinkOpenerProvider)(
      webRoute(config.serverUrl, '/download'),
    );
  } catch (_) {
    if (context.mounted) {
      await showUpdateMessage(context, '暂时无法打开下载页面，请稍后重试');
    }
  }
}

/// 打开本地诊断日志所在文件夹（应用数据目录下的 `logs`，与 Rust 侧
/// `diagnostics::LOG_DIR_NAME` 一致）。文件夹由桥接层初始化时创建。
Future<void> openLogFolder(BuildContext context, WidgetRef ref) async {
  var opened = false;
  try {
    final config = await ref.read(appConfigProvider.future);
    opened = await ref.read(localFolderOpenerProvider)(
      '${config.appDataDir}${Platform.pathSeparator}logs',
    );
  } catch (_) {
    opened = false;
  }
  if (!opened && context.mounted) {
    await showUpdateMessage(context, '暂时无法打开日志文件夹，请稍后重试');
  }
}

/// 安装已校验的更新，并在需要手动拖动 .dmg 时给出提示。
Future<UpdateInstallResult> runUpdateInstall(
  BuildContext context,
  WidgetRef ref,
) async {
  final result = await ref.read(updateControllerProvider.notifier).install();
  if (result == UpdateInstallResult.manualInstallRequired && context.mounted) {
    await showUpdateMessage(context, '请把加密备份拖到“应用程序”');
  }
  return result;
}

/// 更新流程的提示。设置页 / 阻断页没有 Scaffold，用对话框而不是 SnackBar。
Future<void> showUpdateMessage(BuildContext context, String message) {
  return showTokensDialog<void>(
    context: context,
    builder: (dialogContext) {
      final tokens = DesktopTokens.of(dialogContext);
      return TokensDialogShell(
        title: '提示',
        width: DesktopMetrics.dialogWidthSmall,
        headerIcon: LucideIcons.info,
        headerIconBackground: tokens.brand50,
        headerIconColor: tokens.brand600,
        onClose: () => Navigator.of(dialogContext).pop(),
        body: Text(
          message,
          style: TextStyle(
            color: tokens.ink600,
            fontSize: tokens.textSm,
            height: 1.5,
          ),
        ),
        actions: [
          TokensButton(
            key: const Key('update-message-ok'),
            label: '知道了',
            onPressed: () => Navigator.of(dialogContext).pop(),
          ),
        ],
      );
    },
  );
}
