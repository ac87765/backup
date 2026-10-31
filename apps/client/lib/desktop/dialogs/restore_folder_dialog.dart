import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../common/state/transfer_controller.dart';
import '../../common/widgets/app_dialog.dart';
import '../../common/widgets/controls.dart';
import '../../core/errors/bridge_error_text.dart';
import '../../core/platform/file_picker.dart';
import '../../core/rust/models.dart';
import '../../core/theme/design_tokens.dart';

/// 下载云端文件夹到电脑（restore.html）。
///
/// 文件列表的文件夹菜单“下载到电脑…”与备份文件夹卡片的“恢复到电脑…”
/// 共用。成功开始后返回 true，由调用方打开传输面板。
Future<bool> showRestoreFolderDialog({
  required BuildContext context,
  required String folderId,
  required String folderName,
  String? detail,
}) async {
  final result = await showTokensDialog<bool>(
    context: context,
    builder: (dialogContext) => RestoreFolderDialog(
      folderId: folderId,
      folderName: folderName,
      detail: detail,
    ),
  );
  return result ?? false;
}

class RestoreFolderDialog extends ConsumerStatefulWidget {
  const RestoreFolderDialog({
    required this.folderId,
    required this.folderName,
    this.detail,
    super.key,
  });

  final String folderId;
  final String folderName;

  /// 例如“2,140 个文件 · 8.7 GB · 含 14 个子文件夹”；桥接目前不返回递归统计。
  final String? detail;

  @override
  ConsumerState<RestoreFolderDialog> createState() =>
      _RestoreFolderDialogState();
}

class _RestoreFolderDialogState extends ConsumerState<RestoreFolderDialog> {
  String? _destination;
  ConflictPolicyDto _policy = ConflictPolicyDto.keepBoth;
  bool _submitting = false;
  String? _error;

  Future<void> _pickDestination() async {
    final path = await ref.read(filePickerProvider).pickDirectory();
    if (path == null || !mounted) {
      return;
    }
    setState(() => _destination = path);
  }

  Future<void> _start() async {
    final destination = _destination;
    if (destination == null) {
      return;
    }
    setState(() {
      _submitting = true;
      _error = null;
    });
    try {
      await ref
          .read(transferControllerProvider.notifier)
          .startDownloadFolder(
            folderId: widget.folderId,
            folderName: widget.folderName,
            destinationRoot: destination,
            policy: _policy,
          );
      if (!mounted) {
        return;
      }
      Navigator.of(context).pop(true);
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _submitting = false;
        _error = bridgeErrorText(error);
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final destination = _destination;
    return TokensDialogShell(
      title: '下载文件夹到电脑',
      width: DesktopMetrics.dialogWidth,
      headerIcon: LucideIcons.download,
      headerIconBackground: tokens.fileFolderBg,
      headerIconColor: tokens.brand600,
      onClose: _submitting ? () {} : () => Navigator.of(context).pop(false),
      body: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
            decoration: BoxDecoration(
              color: tokens.canvas,
              borderRadius: BorderRadius.circular(tokens.radiusControl),
            ),
            child: Row(
              children: [
                Container(
                  width: 28,
                  height: 28,
                  decoration: BoxDecoration(
                    color: tokens.fileFolderBg,
                    borderRadius: BorderRadius.circular(6),
                  ),
                  child: Icon(
                    LucideIcons.folder,
                    size: 16,
                    color: tokens.brand600,
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        widget.folderName,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          color: tokens.ink900,
                          fontSize: tokens.textSm,
                          fontWeight: FontWeight.w500,
                        ),
                      ),
                      if (widget.detail != null) ...[
                        const SizedBox(height: 2),
                        Text(
                          widget.detail!,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            color: tokens.ink500,
                            fontSize: tokens.textXs,
                          ),
                        ),
                      ],
                    ],
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(height: 18),
          Text(
            '保存到',
            style: TextStyle(
              color: tokens.ink900,
              fontSize: tokens.textSm,
              fontWeight: FontWeight.w600,
            ),
          ),
          const SizedBox(height: 6),
          Row(
            children: [
              Expanded(
                child: Container(
                  height: DesktopMetrics.inputHeight,
                  padding: EdgeInsets.symmetric(horizontal: tokens.space3),
                  decoration: BoxDecoration(
                    color: tokens.surface,
                    border: Border.all(color: tokens.lineStrong),
                    borderRadius: BorderRadius.circular(tokens.radiusControl),
                  ),
                  child: Row(
                    children: [
                      Icon(
                        LucideIcons.hardDrive,
                        size: 16,
                        color: tokens.ink400,
                      ),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          destination ?? '选择保存位置',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            color: destination == null
                                ? tokens.ink400
                                : tokens.ink900,
                            fontSize: tokens.textXs,
                            fontFamily: destination == null
                                ? null
                                : DesktopMetrics.monoFamily,
                            fontFamilyFallback: destination == null
                                ? null
                                : DesktopMetrics.monoFallback,
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
              const SizedBox(width: 8),
              TokensButton(
                key: const Key('restore-pick-destination'),
                label: '选择…',
                onPressed: _submitting ? null : _pickDestination,
              ),
            ],
          ),
          const SizedBox(height: 6),
          Text(
            destination == null
                ? '将保留云端文件夹的完整目录结构。'
                : '将创建 ${_joined(destination)}，并保留其中的目录结构。',
            style: TextStyle(color: tokens.ink500, fontSize: tokens.textXs),
          ),
          const SizedBox(height: 18),
          Text(
            '如果电脑上已有同名文件',
            style: TextStyle(
              color: tokens.ink900,
              fontSize: tokens.textSm,
              fontWeight: FontWeight.w600,
            ),
          ),
          const SizedBox(height: 8),
          _PolicyOption(
            key: const Key('restore-conflict-keepBoth'),
            tokens: tokens,
            title: '保留两份',
            description: '下载的文件改名为“名称 (1).jpg”',
            selected: _policy == ConflictPolicyDto.keepBoth,
            onTap: _submitting
                ? null
                : () => setState(() => _policy = ConflictPolicyDto.keepBoth),
          ),
          const SizedBox(height: 8),
          _PolicyOption(
            key: const Key('restore-conflict-replace'),
            tokens: tokens,
            title: '替换',
            description: '用云端的版本覆盖电脑上的文件',
            selected: _policy == ConflictPolicyDto.replace,
            onTap: _submitting
                ? null
                : () => setState(() => _policy = ConflictPolicyDto.replace),
          ),
          const SizedBox(height: 8),
          _PolicyOption(
            key: const Key('restore-conflict-skip'),
            tokens: tokens,
            title: '跳过',
            description: '保留电脑上的文件，不下载',
            selected: _policy == ConflictPolicyDto.skip,
            onTap: _submitting
                ? null
                : () => setState(() => _policy = ConflictPolicyDto.skip),
          ),
          if (_error != null) ...[
            const SizedBox(height: 12),
            Text(
              _error!,
              style: TextStyle(
                color: tokens.danger700,
                fontSize: tokens.textXs,
              ),
            ),
          ],
        ],
      ),
      actions: [
        TokensButton(
          label: '取消',
          onPressed: _submitting
              ? null
              : () => Navigator.of(context).pop(false),
        ),
        TokensButton(
          key: const Key('restore-start'),
          label: _submitting ? '开始中…' : '开始下载',
          icon: LucideIcons.download,
          variant: TokensButtonVariant.primary,
          onPressed: destination == null || _submitting ? null : _start,
        ),
      ],
    );
  }

  String _joined(String destination) {
    final separator = Platform.pathSeparator;
    final trimmed = destination.endsWith(separator)
        ? destination.substring(0, destination.length - 1)
        : destination;
    // 云端文件夹展示名可能带“父 / 子”路径，实际落盘只用最后一级。
    final leaf = widget.folderName.split(' / ').last;
    return '$trimmed$separator$leaf';
  }
}

class _PolicyOption extends StatelessWidget {
  const _PolicyOption({
    required this.tokens,
    required this.title,
    required this.description,
    required this.selected,
    required this.onTap,
    super.key,
  });

  final DesktopTokens tokens;
  final String title;
  final String description;
  final bool selected;
  final VoidCallback? onTap;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      onTap: onTap,
      borderRadius: BorderRadius.circular(tokens.radiusControl),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
        decoration: BoxDecoration(
          color: selected ? tokens.brand50 : tokens.surface,
          border: Border.all(color: selected ? tokens.brand600 : tokens.line),
          borderRadius: BorderRadius.circular(tokens.radiusControl),
        ),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Container(
              width: 18,
              height: 18,
              decoration: BoxDecoration(
                color: tokens.surface,
                shape: BoxShape.circle,
                border: Border.all(
                  color: selected ? tokens.brand600 : tokens.lineStrong,
                  width: selected ? 5 : 1.5,
                ),
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    title,
                    style: TextStyle(
                      color: tokens.ink900,
                      fontSize: tokens.textSm,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                  const SizedBox(height: 2),
                  Text(
                    description,
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
      ),
    );
  }
}
