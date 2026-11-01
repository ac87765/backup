import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../common/state/transfer_controller.dart';
import '../../common/widgets/app_dialog.dart';
import '../../common/widgets/controls.dart';
import '../../core/clock.dart';
import '../../core/format/format.dart';
import '../../core/platform/file_picker.dart';
import '../../core/platform/local_folders.dart';
import '../../core/providers/app_providers.dart';
import '../../core/rust/models.dart';
import '../../core/theme/design_tokens.dart';
import '../state/wechat_controller.dart';
import '../state/wechat_status.dart';

/// 微信恢复对话框（wechat.html 的 restore 状态，宽 560）。
///
/// 只使用带完成标记的数据库快照；微信运行中时保持对话框打开并提示先退出。
Future<bool> showWechatRestoreDialog({
  required BuildContext context,
  required WechatAccountDto account,
}) async {
  final result = await showTokensDialog<bool>(
    context: context,
    builder: (dialogContext) => WechatRestoreDialog(account: account),
  );
  return result ?? false;
}

class WechatRestoreDialog extends ConsumerStatefulWidget {
  const WechatRestoreDialog({required this.account, super.key});

  final WechatAccountDto account;

  @override
  ConsumerState<WechatRestoreDialog> createState() =>
      _WechatRestoreDialogState();
}

class _WechatRestoreDialogState extends ConsumerState<WechatRestoreDialog> {
  List<WechatRestoreCategoryDto> _categories =
      const <WechatRestoreCategoryDto>[];
  List<WechatDatabaseSnapshotDto> _snapshots =
      const <WechatDatabaseSnapshotDto>[];
  WechatDatabaseSnapshotDto? _selectedSnapshot;
  String? _dataRoot;
  bool _restoreFiles = true;
  bool _restoreDatabase = true;
  bool _loading = true;
  bool _submitting = false;
  String? _error;

  List<WechatRestoreCategoryDto> get _fileCategories => _categories
      .where(
        (item) =>
            item.available && item.category != WechatCategoryDto.databases,
      )
      .toList(growable: false);

  @override
  void initState() {
    super.initState();
    Future<void>.microtask(_load);
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final bridge = ref.read(nativeBridgeProvider);
      final account = wechatAccountRef(widget.account);
      final results = await Future.wait<Object>([
        bridge.wechatRestoreCategories(account),
        bridge.wechatDatabaseSnapshots(account),
      ]);
      if (!mounted) {
        return;
      }
      final categories = results[0] as List<WechatRestoreCategoryDto>;
      final snapshots =
          (results[1] as List<WechatDatabaseSnapshotDto>)
              .where((snapshot) => snapshot.complete)
              .toList(growable: false)
            ..sort(
              (left, right) => right.snapshotAt.compareTo(left.snapshotAt),
            );
      setState(() {
        _categories = categories;
        _snapshots = snapshots;
        _selectedSnapshot = snapshots.isEmpty ? null : snapshots.first;
        _restoreFiles = categories.any(
          (item) =>
              item.available && item.category != WechatCategoryDto.databases,
        );
        _restoreDatabase = snapshots.isNotEmpty;
        _loading = false;
      });
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _loading = false;
        _error = wechatRestoreErrorText(error);
      });
    }
  }

  bool get _accountDirectoryExists =>
      ref.read(directoryExistsProvider)(widget.account.accountDir);

  Future<void> _pickDataRoot() async {
    final path = await ref.read(filePickerProvider).pickDirectory();
    if (path == null || !mounted) {
      return;
    }
    setState(() {
      _dataRoot = path;
      _error = null;
    });
  }

  Future<void> _start() async {
    final fileCategories = _fileCategories
        .map((item) => item.category)
        .toList(growable: false);
    if (_restoreDatabase && _selectedSnapshot == null) {
      return;
    }
    if (!_restoreFiles || fileCategories.isEmpty) {
      if (!_restoreDatabase) {
        return;
      }
    }
    final dataRoot = _dataRoot ?? widget.account.dataRoot;
    if (!_accountDirectoryExists && _dataRoot == null) {
      setState(() => _error = '请选择微信数据目录（包含 xwechat_files 或 WeChat Files 的目录）');
      return;
    }
    setState(() {
      _submitting = true;
      _error = null;
    });
    try {
      await ref
          .read(transferControllerProvider.notifier)
          .startWechatRestore(
            wechatRestoreRequest(
              account: widget.account,
              categories: _restoreFiles
                  ? fileCategories
                  : const <WechatCategoryDto>[],
              restoreDatabase: _restoreDatabase,
              dataRoot: dataRoot,
              snapshot: _restoreDatabase
                  ? WechatSnapshotSelectionDto(
                      deviceName: _selectedSnapshot!.deviceName,
                      timestamp: _selectedSnapshot!.timestamp,
                    )
                  : null,
            ),
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
        _error = wechatRestoreErrorText(error);
      });
    }
  }

  bool get _canStart {
    if (_loading || _submitting) {
      return false;
    }
    if (!_accountDirectoryExists && _dataRoot == null) {
      return false;
    }
    if (_restoreDatabase && _selectedSnapshot == null) {
      return false;
    }
    return (_restoreFiles && _fileCategories.isNotEmpty) ||
        (_restoreDatabase && _snapshots.isNotEmpty);
  }

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final now = ref.read(clockProvider)();
    final exists = _accountDirectoryExists;
    final dataRoot = _dataRoot ?? widget.account.dataRoot;
    return TokensDialogShell(
      title: '恢复到微信',
      width: DesktopMetrics.backupFolderDialogWidth,
      headerIcon: LucideIcons.rotateCcw,
      headerIconBackground: tokens.success50,
      headerIconColor: tokens.success600,
      onClose: _submitting ? () {} : () => Navigator.of(context).pop(false),
      body: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const TokensNote(
            text: '请先完全退出微信（包括托盘图标）。恢复中如果微信启动，会自动暂停。',
            icon: LucideIcons.alertTriangle,
            tone: NoteTone.warn,
          ),
          SizedBox(height: tokens.space4),
          _FieldLabel(tokens: tokens, text: '账号'),
          SizedBox(height: tokens.space2),
          Container(
            height: DesktopMetrics.inputHeight,
            padding: EdgeInsets.symmetric(horizontal: tokens.space3),
            decoration: BoxDecoration(
              color: tokens.surface,
              border: Border.all(color: tokens.lineStrong),
              borderRadius: BorderRadius.circular(tokens.radiusControl),
            ),
            child: Row(
              children: [
                Icon(LucideIcons.messageCircle, size: 16, color: tokens.ink400),
                SizedBox(width: tokens.space2),
                Flexible(
                  child: Text(
                    widget.account.note?.isNotEmpty == true
                        ? widget.account.note!
                        : widget.account.accountName,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      color: tokens.ink900,
                      fontSize: tokens.textSm,
                    ),
                  ),
                ),
                const Spacer(),
                Flexible(
                  child: Text(
                    dataRoot,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    textAlign: TextAlign.right,
                    style: TextStyle(
                      color: tokens.ink500,
                      fontSize: tokens.textXs,
                      fontFamily: DesktopMetrics.monoFamily,
                      fontFamilyFallback: DesktopMetrics.monoFallback,
                    ),
                  ),
                ),
              ],
            ),
          ),
          if (!exists) ...[
            SizedBox(height: tokens.space2),
            Row(
              children: [
                Expanded(
                  child: Text(
                    _dataRoot == null
                        ? '这台电脑上还没有这个账号目录。请选择微信数据目录（登录一次微信后退出也行）。'
                        : '将恢复到 $_dataRoot/${widget.account.accountName}',
                    style: TextStyle(
                      color: _dataRoot == null
                          ? tokens.warningTextStrong
                          : tokens.ink500,
                      fontSize: tokens.textXs,
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                TokensButton(
                  key: const Key('wechat-restore-pick-data-root'),
                  label: '选择微信数据目录…',
                  onPressed: _submitting ? null : _pickDataRoot,
                ),
              ],
            ),
          ],
          SizedBox(height: tokens.space4),
          _FieldLabel(tokens: tokens, text: '恢复内容'),
          SizedBox(height: tokens.space2),
          Container(
            decoration: BoxDecoration(
              border: Border.all(color: tokens.line),
              borderRadius: BorderRadius.circular(tokens.radiusControl),
            ),
            child: Column(
              children: [
                _RestoreOptionRow(
                  key: const Key('wechat-restore-files'),
                  tokens: tokens,
                  selected: _restoreFiles,
                  enabled: !_loading && _fileCategories.isNotEmpty,
                  title: '聊天文件、图片与视频',
                  detail: '只补回缺少的文件，已有的不覆盖',
                  onTap: _submitting || _fileCategories.isEmpty
                      ? null
                      : () => setState(() => _restoreFiles = !_restoreFiles),
                  showBorder: true,
                ),
                _RestoreOptionRow(
                  key: const Key('wechat-restore-database'),
                  tokens: tokens,
                  selected: _restoreDatabase,
                  enabled: !_loading && _snapshots.isNotEmpty,
                  title: '聊天记录数据库',
                  detail: '选择快照',
                  onTap: _submitting || _snapshots.isEmpty
                      ? null
                      : () => setState(
                          () => _restoreDatabase = !_restoreDatabase,
                        ),
                  showBorder: false,
                ),
              ],
            ),
          ),
          SizedBox(height: tokens.space4),
          _FieldLabel(tokens: tokens, text: '数据库快照'),
          SizedBox(height: tokens.space2),
          if (_loading)
            Padding(
              padding: EdgeInsets.symmetric(vertical: tokens.space5),
              child: Center(
                child: SizedBox(
                  width: 20,
                  height: 20,
                  child: CircularProgressIndicator(
                    strokeWidth: 2,
                    color: tokens.brand600,
                  ),
                ),
              ),
            )
          else if (_snapshots.isEmpty)
            Text(
              '还没有可恢复的完整数据库快照。',
              style: TextStyle(color: tokens.ink500, fontSize: tokens.textXs),
            )
          else
            Container(
              decoration: BoxDecoration(
                border: Border.all(color: tokens.line),
                borderRadius: BorderRadius.circular(tokens.radiusControl),
              ),
              child: Column(
                children: [
                  for (var index = 0; index < _snapshots.length; index += 1)
                    _SnapshotRow(
                      key: ValueKey<String>(
                        'wechat-snapshot-${_snapshots[index].timestamp}',
                      ),
                      tokens: tokens,
                      snapshot: _snapshots[index],
                      selected:
                          _selectedSnapshot?.timestamp ==
                          _snapshots[index].timestamp,
                      latest: index == 0,
                      showBorder: index != _snapshots.length - 1,
                      onTap:
                          _submitting ||
                              !_restoreDatabase ||
                              !_snapshots[index].complete
                          ? null
                          : () => setState(
                              () => _selectedSnapshot = _snapshots[index],
                            ),
                      label: formatDateTime(
                        DateTime.fromMillisecondsSinceEpoch(
                          _snapshots[index].snapshotAt * 1000,
                        ),
                        now: now,
                      ),
                    ),
                ],
              ),
            ),
          SizedBox(height: tokens.space2),
          Text(
            '恢复后，这份快照之后收到的新消息在这台电脑上将看不到。现有数据库会改名保留在原处（db_storage.before-restore-…），随时可以改回。',
            style: TextStyle(
              color: tokens.ink500,
              fontSize: tokens.textXs,
              height: 1.5,
            ),
          ),
          if (_error != null) ...[
            SizedBox(height: tokens.space2),
            Text(
              _error!,
              key: const Key('wechat-restore-error'),
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
          key: const Key('wechat-restore-start'),
          label: _submitting ? '正在开始…' : '退出微信后开始恢复',
          variant: TokensButtonVariant.primary,
          busy: _submitting,
          onPressed: _canStart ? _start : null,
        ),
      ],
    );
  }
}

class _FieldLabel extends StatelessWidget {
  const _FieldLabel({required this.tokens, required this.text});

  final DesktopTokens tokens;
  final String text;

  @override
  Widget build(BuildContext context) {
    return Text(
      text,
      style: TextStyle(
        color: tokens.ink900,
        fontSize: tokens.textSm,
        fontWeight: FontWeight.w600,
      ),
    );
  }
}

class _RestoreOptionRow extends StatelessWidget {
  const _RestoreOptionRow({
    required this.tokens,
    required this.selected,
    required this.enabled,
    required this.title,
    required this.detail,
    required this.onTap,
    required this.showBorder,
    super.key,
  });

  final DesktopTokens tokens;
  final bool selected;
  final bool enabled;
  final String title;
  final String detail;
  final VoidCallback? onTap;
  final bool showBorder;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      onTap: enabled ? onTap : null,
      child: Container(
        height: 40,
        padding: EdgeInsets.symmetric(horizontal: tokens.space3),
        decoration: BoxDecoration(
          border: showBorder
              ? Border(bottom: BorderSide(color: tokens.line))
              : null,
        ),
        child: Row(
          children: [
            TokensCheck(value: selected, enabled: enabled),
            SizedBox(width: tokens.space3),
            Expanded(
              child: Text(
                title,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  color: enabled ? tokens.ink900 : tokens.ink500,
                  fontSize: tokens.textSm,
                ),
              ),
            ),
            const Spacer(),
            Flexible(
              child: Text(
                detail,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: tokens.ink500, fontSize: tokens.textXs),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _SnapshotRow extends StatelessWidget {
  const _SnapshotRow({
    required this.tokens,
    required this.snapshot,
    required this.selected,
    required this.latest,
    required this.showBorder,
    required this.onTap,
    required this.label,
    super.key,
  });

  final DesktopTokens tokens;
  final WechatDatabaseSnapshotDto snapshot;
  final bool selected;
  final bool latest;
  final bool showBorder;
  final VoidCallback? onTap;
  final String label;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      onTap: onTap,
      child: Container(
        height: 40,
        padding: EdgeInsets.symmetric(horizontal: tokens.space3),
        decoration: BoxDecoration(
          border: showBorder
              ? Border(bottom: BorderSide(color: tokens.line))
              : null,
        ),
        child: Row(
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
            SizedBox(width: tokens.space3),
            Text(
              label,
              style: TextStyle(color: tokens.ink900, fontSize: tokens.textSm),
            ),
            const Spacer(),
            Text(
              '${formatBytes(snapshot.totalBytes)}${latest ? ' · 最新' : ''}',
              style: TextStyle(color: tokens.ink500, fontSize: tokens.textXs),
            ),
          ],
        ),
      ),
    );
  }
}
