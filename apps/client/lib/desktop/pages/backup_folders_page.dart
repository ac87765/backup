import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../common/dialogs/confirm_dialog.dart';
import '../../common/state/transfer_controller.dart';
import '../../common/widgets/controls.dart';
import '../../common/widgets/file_icon.dart';
import '../../common/widgets/popover_menu.dart';
import '../../core/clock.dart';
import '../../core/format/format.dart';
import '../../core/providers/app_providers.dart';
import '../../core/rust/models.dart';
import '../../core/theme/design_tokens.dart';
import '../dialogs/backup_folder_dialog.dart';
import '../dialogs/restore_folder_dialog.dart';
import '../state/backup_folder_status.dart';
import '../state/backup_folders_controller.dart';

void _showMessage(BuildContext context, String message) {
  ScaffoldMessenger.maybeOf(context)?.showSnackBar(
    SnackBar(
      content: Text(message),
      behavior: SnackBarBehavior.floating,
      width: 360,
    ),
  );
}

/// 备份文件夹（backup-folders.html 的 default / paused / empty）。
class BackupFoldersPage extends ConsumerWidget {
  const BackupFoldersPage({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final state = ref.watch(backupFoldersProvider);
    return Column(
      children: [
        _Toolbar(state: state),
        Expanded(child: _Content(state: state)),
      ],
    );
  }
}

class _Toolbar extends ConsumerWidget {
  const _Toolbar({required this.state});

  final BackupFoldersState state;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = DesktopTokens.of(context);
    final controller = ref.read(backupFoldersProvider.notifier);
    final hasFolders = state.folders.isNotEmpty;
    return Container(
      height: DesktopMetrics.toolbarHeight,
      padding: const EdgeInsets.symmetric(horizontal: 20),
      decoration: BoxDecoration(
        border: Border(bottom: BorderSide(color: tokens.line)),
      ),
      child: Row(
        children: [
          Text(
            '备份文件夹',
            style: TextStyle(
              color: tokens.ink900,
              fontSize: tokens.textXl,
              fontWeight: FontWeight.w600,
            ),
          ),
          const Spacer(),
          if (state.globalPaused)
            TokensButton(
              key: const Key('backup-resume-all'),
              label: '恢复备份',
              icon: LucideIcons.play,
              onPressed: hasFolders ? controller.resumeAll : null,
            )
          else
            TokensButton(
              key: const Key('backup-pause-all'),
              label: '暂停备份',
              icon: LucideIcons.pause,
              onPressed: hasFolders ? controller.pauseAll : null,
            ),
          if (hasFolders) ...[
            const SizedBox(width: 8),
            TokensButton(
              key: const Key('backup-scan-now'),
              label: '立即备份',
              icon: LucideIcons.refreshCw,
              onPressed: controller.scanAll,
            ),
          ],
          const SizedBox(width: 8),
          TokensButton(
            key: const Key('backup-add-folder'),
            label: '添加文件夹',
            icon: LucideIcons.plus,
            variant: TokensButtonVariant.primary,
            onPressed: () => _addFolder(context, ref),
          ),
        ],
      ),
    );
  }

  Future<void> _addFolder(BuildContext context, WidgetRef ref) async {
    final folder = await showBackupFolderDialog(context: context);
    if (folder == null || !context.mounted) {
      return;
    }
    await ref.read(backupFoldersProvider.notifier).load();
    if (context.mounted) {
      _showMessage(context, '已添加“${folder.localName}”，备份即将开始');
    }
  }
}

class _Content extends ConsumerWidget {
  const _Content({required this.state});

  final BackupFoldersState state;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = DesktopTokens.of(context);
    final now = ref.read(clockProvider)();
    final config = ref.watch(appConfigProvider).value;
    return SingleChildScrollView(
      padding: const EdgeInsets.fromLTRB(28, 24, 28, 24),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text.rich(
            TextSpan(
              children: [
                TextSpan(
                  text: '这些文件夹中新增和修改的文件会自动加密备份到云端。',
                  style: TextStyle(color: tokens.ink600),
                ),
                TextSpan(
                  text: '在电脑上删除文件不会删除云端的备份',
                  style: TextStyle(
                    color: tokens.ink600,
                    fontWeight: FontWeight.w500,
                  ),
                ),
                TextSpan(
                  text: '，修改过的文件会保留旧版本。',
                  style: TextStyle(color: tokens.ink600),
                ),
              ],
            ),
            style: TextStyle(fontSize: tokens.textSm),
          ),
          if (state.globalPaused) ...[
            const SizedBox(height: 20),
            const TokensNote(
              key: Key('backup-paused-banner'),
              text: '备份已暂停。新的改动会在恢复后一起备份。',
              icon: LucideIcons.pause,
              tone: NoteTone.warn,
            ),
          ],
          const SizedBox(height: 20),
          if (state.loading && state.folders.isEmpty)
            const Padding(
              padding: EdgeInsets.symmetric(vertical: 80),
              child: Center(child: CircularProgressIndicator(strokeWidth: 2)),
            )
          else if (state.error != null && state.folders.isEmpty)
            _ErrorState(error: state.error!)
          else if (state.folders.isEmpty)
            _EmptyState()
          else ...[
            for (var index = 0; index < state.folders.length; index += 1) ...[
              if (index > 0) SizedBox(height: tokens.space3),
              _FolderCard(
                folder: state.folders[index],
                state: state,
                now: now,
                deviceName: config?.deviceName ?? '',
              ),
            ],
            const SizedBox(height: 20),
            _ActivityCard(state: state, now: now),
          ],
        ],
      ),
    );
  }
}

class _FolderCard extends ConsumerWidget {
  const _FolderCard({
    required this.folder,
    required this.state,
    required this.now,
    required this.deviceName,
  });

  final BackupFolderDto folder;
  final BackupFoldersState state;
  final DateTime now;
  final String deviceName;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = DesktopTokens.of(context);
    final folderState = state.stateFor(folder.id);
    final status = folderStatusView(
      folder,
      folderState,
      globalPaused: state.globalPaused,
      needsUnlock:
          state.summary?.kind == BackupSummaryStatusKindDto.needsUnlock,
    );
    final cloudPath =
        folder.cloudPath ??
        <String>[
          '全部文件',
          '电脑备份',
          if (deviceName.isNotEmpty) deviceName,
          folder.localName,
        ].join(' / ');
    final lastBackup = folderState?.lastBackupAt ?? folder.lastBackupAt;
    return Container(
      key: ValueKey<String>('backup-folder-${folder.id}'),
      padding: const EdgeInsets.fromLTRB(18, 16, 18, 16),
      decoration: BoxDecoration(
        color: tokens.surface,
        border: Border.all(color: tokens.line),
        borderRadius: BorderRadius.circular(tokens.radiusCard),
      ),
      child: Column(
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.center,
            children: [
              _FolderIcon(tokens: tokens, name: folder.localName),
              const SizedBox(width: 14),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      folder.localName,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        color: tokens.ink900,
                        fontSize: tokens.textMd,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                    const SizedBox(height: 4),
                    Row(
                      children: [
                        Flexible(
                          child: Text(
                            folder.localPath,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(
                              color: tokens.ink500,
                              fontSize: tokens.textXs,
                              fontFamily: DesktopMetrics.monoFamily,
                              fontFamilyFallback: DesktopMetrics.monoFallback,
                            ),
                          ),
                        ),
                        Padding(
                          padding: const EdgeInsets.symmetric(horizontal: 6),
                          child: Icon(
                            LucideIcons.arrowRight,
                            size: 12,
                            color: tokens.ink400,
                          ),
                        ),
                        Flexible(
                          child: Text(
                            cloudPath,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(
                              color: tokens.ink500,
                              fontSize: tokens.textXs,
                            ),
                          ),
                        ),
                      ],
                    ),
                  ],
                ),
              ),
              const SizedBox(width: 12),
              // 状态胶囊按内容宽度靠右；剩余宽度全部给左侧名称与路径。
              _StatusPill(status: status),
              const SizedBox(width: 12),
              Text(
                formatBackupTime(lastBackup, now: now),
                style: TextStyle(color: tokens.ink500, fontSize: tokens.textXs),
              ),
              const SizedBox(width: 4),
              Builder(
                builder: (buttonContext) => TokensIconButton(
                  key: ValueKey<String>('backup-folder-more-${folder.id}'),
                  icon: LucideIcons.moreHorizontal,
                  tooltip: '更多操作',
                  onPressed: () => _openMenu(buttonContext, context, ref),
                ),
              ),
            ],
          ),
          if (status.progress != null) ...[
            const SizedBox(height: 10),
            Padding(
              padding: const EdgeInsets.only(left: 54),
              child: _ProgressBar(value: status.progress!),
            ),
          ],
        ],
      ),
    );
  }

  Future<void> _openMenu(
    BuildContext anchorContext,
    BuildContext context,
    WidgetRef ref,
  ) async {
    final controller = ref.read(backupFoldersProvider.notifier);
    final folderState = state.stateFor(folder.id);
    final paused =
        !folder.enabled ||
        folderState?.pauseReason == BackupPauseReasonDto.user;
    await showTokensMenu(
      context: context,
      anchor: anchorOf(anchorContext),
      items: [
        TokensMenuItem(
          label: '立即备份',
          icon: LucideIcons.refreshCw,
          onSelected: () => controller.scanFolder(folder.id),
        ),
        TokensMenuItem(
          label: paused ? '恢复此文件夹' : '暂停此文件夹',
          icon: paused ? LucideIcons.play : LucideIcons.pause,
          onSelected: () => paused
              ? controller.resumeFolder(folder.id)
              : controller.pauseFolder(folder.id),
        ),
        TokensMenuItem(
          label: '编辑排除规则',
          icon: LucideIcons.sliders,
          onSelected: () => _edit(context, ref),
        ),
        TokensMenuItem(
          label: '恢复到电脑…',
          icon: LucideIcons.download,
          onSelected: () => _restore(context, ref),
        ),
        TokensMenuItem(
          label: '移除',
          icon: LucideIcons.trash2,
          danger: true,
          separatorBefore: true,
          onSelected: () => _remove(context, ref),
        ),
      ],
    );
  }

  Future<void> _edit(BuildContext context, WidgetRef ref) async {
    final updated = await showBackupFolderDialog(
      context: context,
      folder: folder,
    );
    if (updated == null || !context.mounted) {
      return;
    }
    await ref.read(backupFoldersProvider.notifier).load();
    if (context.mounted) {
      _showMessage(context, '已保存“${updated.localName}”的排除规则');
    }
  }

  Future<void> _restore(BuildContext context, WidgetRef ref) async {
    var folderId = folder.cloudFolderId;
    if (folderId == null) {
      // 列表快照可能早于首次扫描：向引擎取最新配置。
      try {
        folderId =
            (await ref.read(nativeBridgeProvider).backupFolder(folder.id))
                .cloudFolderId;
      } catch (_) {
        // 读取失败时按“尚无云端文件夹”处理。
      }
    }
    if (folderId == null) {
      if (context.mounted) {
        _showMessage(context, '该文件夹还没有上传内容，暂无可恢复的文件');
      }
      return;
    }
    if (!context.mounted) {
      return;
    }
    final started = await showRestoreFolderDialog(
      context: context,
      folderId: folderId,
      folderName: folder.localName,
    );
    if (started && context.mounted) {
      ref.read(transferPanelOpenProvider.notifier).open();
      _showMessage(context, '已开始下载“${folder.localName}”');
    }
  }

  Future<void> _remove(BuildContext context, WidgetRef ref) async {
    final confirmed = await showConfirmDialog(
      context: context,
      title: '移除备份文件夹？',
      message: '“${folder.localName}”将从这台电脑移除，之后不再自动备份。',
      detail: '云端已备份的文件会保留；本地文件不受影响。',
      confirmLabel: '移除',
      danger: true,
      icon: LucideIcons.trash2,
      iconWarn: false,
    );
    if (!confirmed || !context.mounted) {
      return;
    }
    await ref.read(backupFoldersProvider.notifier).removeFolder(folder.id);
    if (context.mounted) {
      _showMessage(context, '已移除“${folder.localName}”，云端文件保留');
    }
  }
}

class _FolderIcon extends StatelessWidget {
  const _FolderIcon({required this.tokens, required this.name});

  final DesktopTokens tokens;
  final String name;

  @override
  Widget build(BuildContext context) {
    final (icon, color) = _style();
    return Container(
      width: 40,
      height: 40,
      decoration: BoxDecoration(
        color: tokens.brand50,
        borderRadius: BorderRadius.circular(10),
      ),
      child: Icon(icon, size: 20, color: color),
    );
  }

  (IconData, Color) _style() {
    final lower = name.toLowerCase();
    if (lower.contains('照片') ||
        lower.contains('图片') ||
        lower.contains('picture')) {
      return (LucideIcons.image, tokens.brand600);
    }
    if (lower.contains('文档') || lower.contains('document')) {
      return (LucideIcons.fileText, tokens.brand600);
    }
    return (LucideIcons.folder, tokens.brand600);
  }
}

class _StatusPill extends StatelessWidget {
  const _StatusPill({required this.status});

  final FolderStatusView status;

  @override
  Widget build(BuildContext context) {
    final (tone, icon) = switch (status.tone) {
      FolderStatusTone.ok => (PillTone.ok, LucideIcons.check),
      FolderStatusTone.busy => (PillTone.busy, LucideIcons.cloudUpload),
      FolderStatusTone.paused => (PillTone.paused, LucideIcons.pause),
      FolderStatusTone.warn => (PillTone.warn, LucideIcons.alertTriangle),
      FolderStatusTone.needsUnlock => (PillTone.warn, LucideIcons.lock),
    };
    return TokensPill(label: status.label, tone: tone, icon: icon);
  }
}

class _ProgressBar extends StatelessWidget {
  const _ProgressBar({required this.value});

  final double value;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Container(
      height: 4,
      decoration: BoxDecoration(
        color: tokens.canvasStrong,
        borderRadius: BorderRadius.circular(2),
      ),
      clipBehavior: Clip.antiAlias,
      child: Align(
        alignment: Alignment.centerLeft,
        child: FractionallySizedBox(
          widthFactor: value,
          child: Container(color: tokens.brand600),
        ),
      ),
    );
  }
}

class _ActivityCard extends StatelessWidget {
  const _ActivityCard({required this.state, required this.now});

  final BackupFoldersState state;
  final DateTime now;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final folderNames = <String, String>{
      for (final folder in state.folders) folder.id: folder.localName,
    };
    return Container(
      decoration: BoxDecoration(
        color: tokens.surface,
        border: Border.all(color: tokens.line),
        borderRadius: BorderRadius.circular(tokens.radiusCard),
      ),
      child: Column(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(16, 14, 16, 14),
            child: Row(
              children: [
                Text(
                  '最近活动',
                  style: TextStyle(
                    color: tokens.ink900,
                    fontSize: tokens.textSm,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                const Spacer(),
                Text(
                  '只保存在这台电脑上',
                  style: TextStyle(
                    color: tokens.ink500,
                    fontSize: tokens.textXs,
                  ),
                ),
              ],
            ),
          ),
          Container(height: 1, color: tokens.line),
          if (state.activities.isEmpty)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 20),
              child: Text(
                '还没有备份活动',
                style: TextStyle(color: tokens.ink500, fontSize: tokens.textXs),
              ),
            )
          else
            for (var index = 0; index < state.activities.length; index += 1)
              _ActivityRow(
                activity: state.activities[index],
                folderName: folderNames[state.activities[index].folderId],
                now: now,
                showBorder: index != state.activities.length - 1,
              ),
        ],
      ),
    );
  }
}

class _ActivityRow extends StatelessWidget {
  const _ActivityRow({
    required this.activity,
    required this.folderName,
    required this.now,
    required this.showBorder,
  });

  final BackupActivityDto activity;
  final String? folderName;
  final DateTime now;
  final bool showBorder;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final name = baseName(activity.relativePath);
    final (label, color) = _status(tokens);
    return Container(
      height: 40,
      padding: const EdgeInsets.symmetric(horizontal: 16),
      decoration: BoxDecoration(
        border: showBorder
            ? Border(bottom: BorderSide(color: tokens.line))
            : null,
      ),
      child: Row(
        children: [
          FileIconTile(name: name, size: 24, iconSize: 14),
          const SizedBox(width: 12),
          // 名称与所属文件夹占满左侧，状态文字靠右（Spacer 与 Flexible 会平分宽度）。
          Expanded(
            child: Row(
              children: [
                Flexible(
                  child: Text(
                    name,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      color: tokens.ink900,
                      fontSize: tokens.textSm,
                    ),
                  ),
                ),
                if (folderName != null) ...[
                  const SizedBox(width: 10),
                  Flexible(
                    child: Text(
                      folderName!,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        color: tokens.ink500,
                        fontSize: tokens.textXs,
                      ),
                    ),
                  ),
                ],
              ],
            ),
          ),
          const SizedBox(width: 12),
          Text(
            label,
            style: TextStyle(
              color: color ?? tokens.ink500,
              fontSize: tokens.textXs,
            ),
          ),
        ],
      ),
    );
  }

  (String, Color?) _status(DesktopTokens tokens) {
    return switch (activity.kind) {
      BackupActivityKindDto.added => (
        '新增 · ${formatRelativeTime(activity.occurredAt, now: now)}',
        null,
      ),
      BackupActivityKindDto.newVersion => (
        '新版本 · ${formatRelativeTime(activity.occurredAt, now: now)}',
        null,
      ),
      BackupActivityKindDto.unchanged => (
        '内容未变，已跳过 · ${formatClockTime(activity.occurredAt)}',
        null,
      ),
      BackupActivityKindDto.retryLater => ('被其他程序占用，稍后重试', tokens.warning600),
      BackupActivityKindDto.failed => (
        activity.message?.isNotEmpty == true ? activity.message! : '备份失败',
        tokens.danger700,
      ),
    };
  }
}

class _EmptyState extends ConsumerWidget {
  const _EmptyState();

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = DesktopTokens.of(context);
    return Container(
      padding: const EdgeInsets.symmetric(vertical: 72, horizontal: 24),
      decoration: BoxDecoration(
        color: tokens.surface,
        border: Border.all(color: tokens.line),
        borderRadius: BorderRadius.circular(tokens.radiusCard),
      ),
      child: Column(
        children: [
          Container(
            width: 56,
            height: 56,
            decoration: BoxDecoration(
              color: tokens.brand50,
              borderRadius: BorderRadius.circular(14),
            ),
            child: Icon(
              LucideIcons.cloudUpload,
              size: 28,
              color: tokens.brand600,
            ),
          ),
          const SizedBox(height: 12),
          Text(
            '让电脑上的文件自动备份',
            style: TextStyle(
              color: tokens.ink900,
              fontSize: tokens.textLg,
              fontWeight: FontWeight.w600,
            ),
          ),
          const SizedBox(height: 6),
          ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 420),
            child: Text(
              '选择文件夹后，其中新增和修改的文件会在后台自动加密上传。关闭窗口也会继续。',
              textAlign: TextAlign.center,
              style: TextStyle(
                color: tokens.ink500,
                fontSize: tokens.textSm,
                height: 1.5,
              ),
            ),
          ),
          const SizedBox(height: 20),
          TokensButton(
            key: const Key('backup-empty-add'),
            label: '添加文件夹',
            icon: LucideIcons.plus,
            large: true,
            variant: TokensButtonVariant.primary,
            onPressed: () async {
              final folder = await showBackupFolderDialog(context: context);
              if (folder == null || !context.mounted) {
                return;
              }
              await ref.read(backupFoldersProvider.notifier).load();
            },
          ),
        ],
      ),
    );
  }
}

class _ErrorState extends ConsumerWidget {
  const _ErrorState({required this.error});

  final String error;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = DesktopTokens.of(context);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 64),
      child: Column(
        children: [
          Icon(LucideIcons.alertTriangle, size: 28, color: tokens.danger600),
          const SizedBox(height: 12),
          Text(
            error,
            textAlign: TextAlign.center,
            style: TextStyle(color: tokens.ink600, fontSize: tokens.textSm),
          ),
          const SizedBox(height: 16),
          TokensButton(
            label: '重试',
            onPressed: () => ref.read(backupFoldersProvider.notifier).load(),
          ),
        ],
      ),
    );
  }
}
