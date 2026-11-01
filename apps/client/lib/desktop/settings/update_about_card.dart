import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../common/widgets/app_dialog.dart';
import '../../common/widgets/controls.dart';
import '../../core/format/format.dart';
import '../../core/theme/design_tokens.dart';
import '../state/update_controller.dart';
import '../widgets/update_actions.dart';

/// “关于”中的更新卡片（design/desktop/update.html 的
/// available / downloading / restart / failed / latest / development /
/// unavailable）。
class UpdateAboutCard extends ConsumerStatefulWidget {
  const UpdateAboutCard({super.key});

  @override
  ConsumerState<UpdateAboutCard> createState() => _UpdateAboutCardState();
}

class _UpdateAboutCardState extends ConsumerState<UpdateAboutCard> {
  String? _promptedVersion;

  @override
  Widget build(BuildContext context) {
    final state = ref.watch(updateControllerProvider);
    final phase = state.phase;
    if (state.cardPostponed &&
        (phase == UpdatePhase.available || phase == UpdatePhase.completed)) {
      return const SizedBox.shrink();
    }
    if (phase == UpdatePhase.downloading) {
      // 重新下载同一个版本后再次完成时要再提示一次。
      _promptedVersion = null;
    }
    if (phase == UpdatePhase.completed) {
      final version = state.info?.version;
      if (version != null && version != _promptedVersion) {
        _promptedVersion = version;
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (mounted) {
            _showRestartDialog(version);
          }
        });
      }
    }
    return switch (phase) {
      UpdatePhase.development => _developmentCard(context, state),
      UpdatePhase.unavailable => _unavailableCard(context, state),
      UpdatePhase.upToDate => _latestCard(context, state),
      UpdatePhase.available ||
      UpdatePhase.completed => _availableCard(context, state),
      UpdatePhase.downloading => _downloadingCard(context, state),
      UpdatePhase.failed => _failedCard(context, state),
      UpdatePhase.idle || UpdatePhase.checking => _idleCard(context, state),
    };
  }

  Widget _availableCard(BuildContext context, UpdateState state) {
    final tokens = DesktopTokens.of(context);
    final info = state.info;
    final version = info?.version ?? '';
    return Container(
      key: const Key('update-card-available'),
      margin: const EdgeInsets.only(top: 20),
      padding: const EdgeInsets.all(16),
      decoration: _freshDecoration(tokens),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _header(
            tokens,
            icon: LucideIcons.download,
            iconColor: tokens.brand600,
            title: '有新版本',
            version: version,
            trailing: info == null
                ? null
                : '${_formatDate(info.releasedAt)} · ${formatPackageBytes(info.size)}',
          ),
          if (info != null && info.notes.isNotEmpty) ...[
            const SizedBox(height: 12),
            for (final note in info.notes)
              Padding(
                padding: const EdgeInsets.only(left: 4, bottom: 4),
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      '•',
                      style: TextStyle(
                        color: tokens.ink600,
                        fontSize: tokens.textSm,
                        height: 1.5,
                      ),
                    ),
                    const SizedBox(width: 8),
                    Expanded(
                      child: Text(
                        note,
                        style: TextStyle(
                          color: tokens.ink600,
                          fontSize: tokens.textSm,
                          height: 1.5,
                        ),
                      ),
                    ),
                  ],
                ),
              ),
          ],
          const SizedBox(height: 10),
          Row(
            children: [
              TokensButton(
                key: const Key('update-download'),
                label: '下载并安装',
                icon: LucideIcons.download,
                variant: TokensButtonVariant.primary,
                busy: state.installing,
                onPressed: state.installing
                    ? null
                    : () => ref
                          .read(updateControllerProvider.notifier)
                          .startDownload(),
              ),
              const SizedBox(width: 8),
              TokensButton(
                key: const Key('update-later'),
                label: '稍后',
                variant: TokensButtonVariant.ghost,
                onPressed: state.installing
                    ? null
                    : () => ref
                          .read(updateControllerProvider.notifier)
                          .postponeCard(),
              ),
              const Spacer(),
              Icon(LucideIcons.shieldCheck, size: 14, color: tokens.success600),
              const SizedBox(width: 6),
              Text(
                '已验证发布签名',
                style: TextStyle(color: tokens.ink500, fontSize: tokens.textXs),
              ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _downloadingCard(BuildContext context, UpdateState state) {
    final tokens = DesktopTokens.of(context);
    final progress = state.download;
    final downloaded = progress?.downloadedBytes ?? 0;
    final total = progress?.totalBytes ?? state.info?.size ?? 0;
    final speed = formatSpeed(progress?.speedBytesPerSecond.toDouble());
    final detail = speed.isEmpty
        ? '${formatPackageBytes(downloaded)} / ${formatPackageBytes(total)}'
        : '${formatPackageBytes(downloaded)} / ${formatPackageBytes(total)} · $speed';
    final fraction = total <= 0
        ? 0.0
        : (downloaded / total).clamp(0.0, 1.0).toDouble();
    return Container(
      key: const Key('update-card-downloading'),
      margin: const EdgeInsets.only(top: 20),
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        border: Border.all(color: tokens.line),
        borderRadius: BorderRadius.circular(10),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _header(
            tokens,
            icon: LucideIcons.download,
            iconColor: tokens.brand600,
            title: '正在下载',
            version: state.info?.version ?? '',
            trailing: detail,
          ),
          const SizedBox(height: 14),
          Container(
            height: 6,
            decoration: BoxDecoration(
              color: tokens.canvasStrong,
              borderRadius: BorderRadius.circular(3),
            ),
            clipBehavior: Clip.antiAlias,
            child: Align(
              alignment: Alignment.centerLeft,
              child: FractionallySizedBox(
                widthFactor: fraction,
                child: Container(color: tokens.brand600),
              ),
            ),
          ),
          const SizedBox(height: 14),
          Row(
            children: [
              Expanded(
                child: Text(
                  '下载完成后会校验文件，备份照常进行。',
                  style: TextStyle(
                    color: tokens.ink500,
                    fontSize: tokens.textXs,
                  ),
                ),
              ),
              const SizedBox(width: 12),
              TokensButton(
                key: const Key('update-cancel'),
                label: '取消',
                onPressed: () => ref
                    .read(updateControllerProvider.notifier)
                    .cancelDownload(),
              ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _failedCard(BuildContext context, UpdateState state) {
    final tokens = DesktopTokens.of(context);
    final version = state.info?.version ?? '';
    final message =
        state.failure ??
        state.download?.error?.message ??
        '下载的文件不完整或被篡改，已删除，没有运行。可能是网络中断或被安全软件修改。';
    return Container(
      key: const Key('update-card-failed'),
      margin: const EdgeInsets.only(top: 20),
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        border: Border.all(color: tokens.danger600.withValues(alpha: 0.35)),
        borderRadius: BorderRadius.circular(10),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Icon(
                LucideIcons.triangleAlert,
                size: 18,
                color: tokens.danger600,
              ),
              const SizedBox(width: 10),
              Text(
                '无法安装 $version',
                style: TextStyle(
                  color: tokens.ink900,
                  fontSize: tokens.textMd,
                  fontWeight: FontWeight.w600,
                ),
              ),
            ],
          ),
          const SizedBox(height: 10),
          Text(
            message,
            style: TextStyle(
              color: tokens.ink500,
              fontSize: tokens.textSm,
              height: 1.5,
            ),
          ),
          const SizedBox(height: 14),
          Row(
            children: [
              TokensButton(
                key: const Key('update-retry'),
                label: '重新下载',
                icon: LucideIcons.refreshCw,
                variant: TokensButtonVariant.primary,
                onPressed: () =>
                    ref.read(updateControllerProvider.notifier).startDownload(),
              ),
              const SizedBox(width: 8),
              TokensButton(
                key: const Key('update-failed-download-page'),
                label: '打开下载页面',
                icon: LucideIcons.externalLink,
                variant: TokensButtonVariant.ghost,
                onPressed: () => openUpdateDownloadPage(context, ref),
              ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _latestCard(BuildContext context, UpdateState state) {
    final tokens = DesktopTokens.of(context);
    return Container(
      key: const Key('update-card-latest'),
      margin: const EdgeInsets.only(top: 20),
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        border: Border.all(color: tokens.line),
        borderRadius: BorderRadius.circular(10),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              Icon(LucideIcons.circleCheck, size: 18, color: tokens.success600),
              const SizedBox(width: 10),
              Text(
                '已是最新版本',
                style: TextStyle(
                  color: tokens.ink900,
                  fontSize: tokens.textMd,
                  fontWeight: FontWeight.w600,
                ),
              ),
              const Spacer(),
              TokensButton(
                key: const Key('update-check'),
                label: '检查更新',
                icon: LucideIcons.refreshCw,
                variant: TokensButtonVariant.ghost,
                onPressed: state.checking
                    ? null
                    : () => ref
                          .read(updateControllerProvider.notifier)
                          .checkNow(),
              ),
            ],
          ),
          const SizedBox(height: 6),
          Padding(
            padding: const EdgeInsets.only(left: 28),
            child: Text(
              '刚刚检查 · 每天自动检查一次',
              style: TextStyle(color: tokens.ink500, fontSize: tokens.textXs),
            ),
          ),
        ],
      ),
    );
  }

  Widget _developmentCard(BuildContext context, UpdateState state) {
    final tokens = DesktopTokens.of(context);
    return Container(
      key: const Key('update-card-development'),
      margin: const EdgeInsets.only(top: 20),
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        border: Border.all(color: tokens.line),
        borderRadius: BorderRadius.circular(10),
      ),
      child: Row(
        children: [
          Icon(LucideIcons.code, size: 18, color: tokens.ink500),
          const SizedBox(width: 10),
          Text(
            '开发版本，不检查更新',
            style: TextStyle(
              color: tokens.ink900,
              fontSize: tokens.textMd,
              fontWeight: FontWeight.w600,
            ),
          ),
        ],
      ),
    );
  }

  Widget _unavailableCard(BuildContext context, UpdateState state) {
    final tokens = DesktopTokens.of(context);
    return Container(
      key: const Key('update-card-unavailable'),
      margin: const EdgeInsets.only(top: 20),
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        border: Border.all(color: tokens.line),
        borderRadius: BorderRadius.circular(10),
      ),
      child: Row(
        children: [
          Icon(LucideIcons.info, size: 18, color: tokens.ink500),
          const SizedBox(width: 10),
          Expanded(
            child: Text(
              '上次检查：无法获取更新信息',
              style: TextStyle(
                color: tokens.ink600,
                fontSize: tokens.textSm,
                height: 1.5,
              ),
            ),
          ),
          TokensButton(
            key: const Key('update-check'),
            label: '检查更新',
            icon: LucideIcons.refreshCw,
            variant: TokensButtonVariant.ghost,
            onPressed: state.checking
                ? null
                : () => ref.read(updateControllerProvider.notifier).checkNow(),
          ),
        ],
      ),
    );
  }

  Widget _idleCard(BuildContext context, UpdateState state) {
    final tokens = DesktopTokens.of(context);
    return Container(
      key: const Key('update-card-idle'),
      margin: const EdgeInsets.only(top: 20),
      padding: const EdgeInsets.all(16),
      decoration: BoxDecoration(
        border: Border.all(color: tokens.line),
        borderRadius: BorderRadius.circular(10),
      ),
      child: Row(
        children: [
          Icon(LucideIcons.refreshCw, size: 18, color: tokens.ink500),
          const SizedBox(width: 10),
          Expanded(
            child: Text(
              state.checking ? '正在检查更新…' : '每天自动检查一次',
              style: TextStyle(color: tokens.ink500, fontSize: tokens.textSm),
            ),
          ),
          TokensButton(
            key: const Key('update-check'),
            label: state.checking ? '正在检查…' : '检查更新',
            icon: LucideIcons.refreshCw,
            variant: TokensButtonVariant.ghost,
            onPressed: state.checking
                ? null
                : () => ref.read(updateControllerProvider.notifier).checkNow(),
          ),
        ],
      ),
    );
  }

  Widget _header(
    DesktopTokens tokens, {
    required IconData icon,
    required Color iconColor,
    required String title,
    required String version,
    String? trailing,
  }) {
    return Row(
      children: [
        Icon(icon, size: 18, color: iconColor),
        const SizedBox(width: 10),
        Text(
          title,
          style: TextStyle(
            color: tokens.ink900,
            fontSize: tokens.textMd,
            fontWeight: FontWeight.w600,
          ),
        ),
        if (version.isNotEmpty) ...[
          const SizedBox(width: 10),
          Container(
            height: 20,
            padding: const EdgeInsets.symmetric(horizontal: 8),
            alignment: Alignment.center,
            decoration: BoxDecoration(
              color: tokens.brand50,
              borderRadius: BorderRadius.circular(999),
            ),
            child: Text(
              version,
              style: TextStyle(
                color: tokens.brand700,
                fontSize: tokens.textXs,
                fontWeight: FontWeight.w600,
              ),
            ),
          ),
        ],
        const SizedBox(width: 12),
        Expanded(
          child: trailing == null
              ? const SizedBox.shrink()
              : Text(
                  trailing,
                  textAlign: TextAlign.end,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    color: tokens.ink500,
                    fontSize: tokens.textXs,
                  ),
                ),
        ),
      ],
    );
  }

  BoxDecoration _freshDecoration(DesktopTokens tokens) {
    return BoxDecoration(
      border: Border.all(color: tokens.brand100),
      borderRadius: BorderRadius.circular(10),
      boxShadow: <BoxShadow>[BoxShadow(color: tokens.brand50, spreadRadius: 3)],
    );
  }

  String _formatDate(String value) {
    final parsed = DateTime.tryParse(value);
    if (parsed == null) {
      return value;
    }
    final local = parsed.toLocal();
    return '${local.year}年${local.month}月${local.day}日';
  }

  Future<void> _showRestartDialog(String version) async {
    await showTokensDialog<void>(
      context: context,
      builder: (dialogContext) {
        final dialogTokens = DesktopTokens.of(dialogContext);
        return TokensDialogShell(
          title: '重新启动以完成更新？',
          width: DesktopMetrics.dialogWidthSmall,
          showClose: false,
          headerIcon: LucideIcons.refreshCw,
          headerIconBackground: dialogTokens.brand50,
          headerIconColor: dialogTokens.brand600,
          onClose: () => Navigator.of(dialogContext).pop(),
          body: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                '$version 已下载并通过校验。加密备份会暂停备份、关闭，安装完成后自动重新打开。',
                style: TextStyle(
                  color: dialogTokens.ink600,
                  fontSize: dialogTokens.textSm,
                  height: 1.5,
                ),
              ),
              const SizedBox(height: 12),
              Text(
                '登录状态、备份文件夹和设置都会保留。未完成的上传会在重新打开后继续。',
                style: TextStyle(
                  color: dialogTokens.ink500,
                  fontSize: dialogTokens.textXs,
                  height: 1.5,
                ),
              ),
            ],
          ),
          actions: [
            TokensButton(
              key: const Key('update-restart-later'),
              label: '稍后',
              onPressed: () => Navigator.of(dialogContext).pop(),
            ),
            TokensButton(
              key: const Key('update-restart-now'),
              label: '立即重新启动',
              variant: TokensButtonVariant.primary,
              onPressed: () {
                Navigator.of(dialogContext).pop();
                if (mounted) {
                  unawaited(runUpdateInstall(context, ref));
                }
              },
            ),
          ],
        );
      },
    );
  }
}
