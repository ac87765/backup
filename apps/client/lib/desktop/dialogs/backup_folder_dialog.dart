import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../common/dialogs/folder_picker_dialog.dart';
import '../../common/state/auth_controller.dart';
import '../../common/widgets/app_dialog.dart';
import '../../common/widgets/controls.dart';
import '../../core/errors/bridge_error_text.dart';
import '../../core/format/format.dart';
import '../../core/platform/file_picker.dart';
import '../../core/platform/local_folders.dart';
import '../../core/providers/app_providers.dart';
import '../../core/rust/models.dart';
import '../../core/theme/design_tokens.dart';

/// 添加 / 编辑备份文件夹（add-folder.html 的 default / quota 状态）。
Future<BackupFolderDto?> showBackupFolderDialog({
  required BuildContext context,
  BackupFolderDto? folder,
  String? initialLocalPath,
}) {
  return showTokensDialog<BackupFolderDto>(
    context: context,
    builder: (dialogContext) =>
        BackupFolderDialog(folder: folder, initialLocalPath: initialLocalPath),
  );
}

class BackupFolderDialog extends ConsumerStatefulWidget {
  const BackupFolderDialog({this.folder, this.initialLocalPath, super.key});

  final BackupFolderDto? folder;
  final String? initialLocalPath;

  @override
  ConsumerState<BackupFolderDialog> createState() => _BackupFolderDialogState();
}

class _BackupFolderDialogState extends ConsumerState<BackupFolderDialog> {
  late String? _localPath = widget.folder?.localPath ?? widget.initialLocalPath;
  late FolderTarget? _cloudTarget = widget.folder?.cloudTargetId == null
      ? null
      : FolderTarget(
          id: widget.folder!.cloudTargetId!,
          name: _targetName(widget.folder!),
          path: widget.folder!.cloudPath,
        );
  late final List<String> _globs = <String>[...?widget.folder?.excludeGlobs];
  late final Set<String> _excludedSubfolders = <String>{
    ...?widget.folder?.excludeSubfolders,
  };
  final Map<String, bool> _include = <String, bool>{};
  List<String> _subfolders = const <String>[];
  BackupPreflightDto? _preflight;
  String? _error;
  bool _loading = false;
  bool _submitting = false;

  bool get _editing => widget.folder != null;

  @override
  void initState() {
    super.initState();
    if (_localPath != null) {
      Future<void>.microtask(_reload);
    }
  }

  String _targetName(BackupFolderDto folder) {
    final path = folder.cloudPath;
    if (path == null || path.isEmpty) {
      return '云端文件夹';
    }
    return path;
  }

  BackupFolderDraftDto _draft() {
    return BackupFolderDraftDto(
      localPath: _localPath ?? '',
      cloudTargetId: _cloudTarget?.id,
      excludeGlobs: List<String>.of(_globs),
      excludeSubfolders: _subfolderExcludes(),
    );
  }

  List<String> _subfolderExcludes() {
    final result = <String>{..._excludedSubfolders};
    for (final name in _subfolders) {
      if (_include[name] == false) {
        result.add(name);
      } else {
        result.remove(name);
      }
    }
    return result.toList(growable: false);
  }

  Future<void> _reload() async {
    final path = _localPath;
    if (path == null) {
      return;
    }
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final inspector = ref.read(localFolderInspectorProvider);
      final names = await inspector.listSubfolders(path);
      final preflight = await ref
          .read(nativeBridgeProvider)
          .backupPreflight(_draft());
      if (!mounted) {
        return;
      }
      setState(() {
        _subfolders = names;
        for (final name in names) {
          _include[name] =
              _include[name] ?? !_excludedSubfolders.contains(name);
        }
        _preflight = preflight;
        _loading = false;
      });
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _loading = false;
        _preflight = null;
        _error = bridgeErrorText(error);
      });
    }
  }

  Future<void> _refreshPreflight() async {
    if (_localPath == null) {
      return;
    }
    try {
      final preflight = await ref
          .read(nativeBridgeProvider)
          .backupPreflight(_draft());
      if (!mounted) {
        return;
      }
      setState(() {
        _preflight = preflight;
        _error = null;
      });
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() => _error = bridgeErrorText(error));
    }
  }

  Future<void> _pickFolder() async {
    final path = await ref.read(filePickerProvider).pickDirectory();
    if (path == null || !mounted) {
      return;
    }
    setState(() {
      _localPath = path;
      _cloudTarget = null;
      _preflight = null;
      _subfolders = const <String>[];
      _include.clear();
      _excludedSubfolders.clear();
    });
    await _reload();
  }

  Future<void> _pickCloudTarget() async {
    final rootId = ref.read(authControllerProvider).account?.rootNodeId;
    if (rootId == null) {
      setState(() => _error = '请先登录并解锁');
      return;
    }
    final target = await showFolderPickerDialog(
      context: context,
      rootId: rootId,
      title: '选择云端目标文件夹',
      confirmLabel: '选择这个文件夹',
    );
    if (target == null || !mounted) {
      return;
    }
    setState(() => _cloudTarget = target);
    await _refreshPreflight();
  }

  Future<void> _addRule() async {
    final rule = await showTokensDialog<String>(
      context: context,
      builder: (dialogContext) => _AddRuleDialog(existing: _globs),
    );
    if (rule == null || !mounted) {
      return;
    }
    setState(() => _globs.add(rule));
    await _refreshPreflight();
  }

  void _removeRule(String rule) {
    setState(() => _globs.remove(rule));
    _refreshPreflight();
  }

  void _toggleSubfolder(String name, bool include) {
    setState(() => _include[name] = include);
    _refreshPreflight();
  }

  Future<void> _submit() async {
    if (!_canSubmit) {
      return;
    }
    setState(() {
      _submitting = true;
      _error = null;
    });
    try {
      final bridge = ref.read(nativeBridgeProvider);
      final folder = _editing
          ? await bridge.backupUpdateFolder(
              widget.folder!.id,
              BackupFolderUpdateDto(
                localPath: _localPath,
                cloudTargetId: _cloudTarget?.id,
                resetCloudTarget: _cloudTarget == null,
                excludeGlobs: List<String>.of(_globs),
                excludeSubfolders: _subfolderExcludes(),
              ),
            )
          : await bridge.backupAddFolder(_draft());
      if (!mounted) {
        return;
      }
      Navigator.of(context).pop(folder);
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

  bool get _quotaExceeded {
    final preflight = _preflight;
    final remaining = preflight?.remainingQuotaBytes;
    return preflight != null &&
        remaining != null &&
        preflight.uploadBytes > remaining;
  }

  bool get _canSubmit =>
      _localPath != null &&
      !_loading &&
      !_submitting &&
      _preflight != null &&
      !_quotaExceeded;

  String? get _cloudPath {
    final target = _cloudTarget;
    if (target != null) {
      return target.path ?? '全部文件 / ${target.name}';
    }
    final path = _localPath;
    if (path == null) {
      return null;
    }
    final device = ref.watch(appConfigProvider).value?.deviceName ?? '';
    return <String>[
      '全部文件',
      '电脑备份',
      if (device.isNotEmpty) device,
      baseName(path),
    ].join(' / ');
  }

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final preflight = _preflight;
    final quota = _quotaExceeded;
    return TokensDialogShell(
      title: _editing ? '编辑备份文件夹' : '添加备份文件夹',
      width: DesktopMetrics.backupFolderDialogWidth,
      headerIcon: _editing ? LucideIcons.folderCog : LucideIcons.folderPlus,
      headerIconBackground: tokens.fileFolderBg,
      headerIconColor: tokens.brand600,
      onClose: _submitting ? () {} : () => Navigator.of(context).pop(),
      body: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _FieldLabel(tokens: tokens, text: '电脑上的文件夹'),
          const SizedBox(height: 6),
          _PathRow(
            tokens: tokens,
            icon: LucideIcons.hardDrive,
            text: _localPath,
            placeholder: '选择要自动备份的文件夹',
            mono: true,
            buttonLabel: '选择…',
            onPressed: _submitting ? null : _pickFolder,
            buttonKey: const Key('backup-pick-folder'),
          ),
          const SizedBox(height: 18),
          _FieldLabel(tokens: tokens, text: '备份到'),
          const SizedBox(height: 6),
          _PathRow(
            tokens: tokens,
            icon: LucideIcons.cloud,
            text: _cloudPath,
            placeholder: '选择本地文件夹后自动设置',
            buttonLabel: '更改',
            onPressed: _localPath == null || _submitting
                ? null
                : _pickCloudTarget,
            buttonKey: const Key('backup-change-target'),
          ),
          const SizedBox(height: 6),
          Text(
            '云端已有同名文件夹时会合并到其中。',
            style: TextStyle(color: tokens.ink500, fontSize: tokens.textXs),
          ),
          const SizedBox(height: 18),
          _FieldLabel(tokens: tokens, text: '不备份这些内容'),
          const SizedBox(height: 8),
          Wrap(
            spacing: 6,
            runSpacing: 6,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              Container(
                key: const Key('exclude-fixed-rule'),
                height: 26,
                padding: const EdgeInsets.symmetric(horizontal: 10),
                decoration: BoxDecoration(
                  color: tokens.canvas,
                  border: Border.all(color: tokens.lineStrong),
                  borderRadius: BorderRadius.circular(13),
                ),
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Icon(LucideIcons.lock, size: 12, color: tokens.ink500),
                    const SizedBox(width: 6),
                    Text(
                      '系统与临时文件',
                      style: TextStyle(
                        color: tokens.ink600,
                        fontSize: tokens.textXs,
                      ),
                    ),
                  ],
                ),
              ),
              for (final rule in _globs)
                _RuleChip(
                  key: ValueKey<String>('exclude-rule-$rule'),
                  tokens: tokens,
                  label: rule,
                  onRemove: _submitting ? null : () => _removeRule(rule),
                ),
              InkWell(
                key: const Key('exclude-rule-add'),
                onTap: _submitting ? null : _addRule,
                borderRadius: BorderRadius.circular(13),
                child: Container(
                  height: 26,
                  padding: const EdgeInsets.symmetric(horizontal: 10),
                  decoration: BoxDecoration(
                    border: Border.all(color: tokens.lineStrong),
                    borderRadius: BorderRadius.circular(13),
                  ),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(LucideIcons.plus, size: 12, color: tokens.brand700),
                      const SizedBox(width: 4),
                      Text(
                        '添加规则',
                        style: TextStyle(
                          color: tokens.brand700,
                          fontSize: tokens.textXs,
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ],
          ),
          if (_subfolders.isNotEmpty) ...[
            const SizedBox(height: 10),
            Container(
              decoration: BoxDecoration(
                border: Border.all(color: tokens.line),
                borderRadius: BorderRadius.circular(tokens.radiusControl),
              ),
              child: Column(
                children: [
                  for (var index = 0; index < _subfolders.length; index += 1)
                    _SubfolderRow(
                      key: ValueKey<String>(
                        'subfolder-toggle-${_subfolders[index]}',
                      ),
                      tokens: tokens,
                      name: _subfolders[index],
                      included: _include[_subfolders[index]] ?? true,
                      showBorder: index != _subfolders.length - 1,
                      onChanged: _submitting
                          ? null
                          : (value) =>
                                _toggleSubfolder(_subfolders[index], value),
                    ),
                ],
              ),
            ),
          ],
          const SizedBox(height: 16),
          if (_loading)
            Text(
              '正在统计…',
              style: TextStyle(color: tokens.ink500, fontSize: tokens.textSm),
            )
          else if (preflight != null)
            _PreflightSummary(
              key: const Key('backup-preflight-summary'),
              tokens: tokens,
              preflight: preflight,
              quotaExceeded: quota,
            ),
          if (preflight != null && preflight.excludedWechatBytes > 0) ...[
            const SizedBox(height: 8),
            Row(
              key: const Key('backup-wechat-excluded-note'),
              children: [
                Icon(LucideIcons.messageCircle, size: 13, color: tokens.ink500),
                const SizedBox(width: 6),
                Expanded(
                  child: Text(
                    '已跳过微信数据（约 ${formatBytes(preflight.excludedWechatBytes)}），由“微信备份”单独处理',
                    style: TextStyle(
                      color: tokens.ink500,
                      fontSize: tokens.textXs,
                    ),
                  ),
                ),
              ],
            ),
          ],
          if (preflight != null) ...[
            const SizedBox(height: 12),
            if (quota)
              TokensNote(
                key: const Key('backup-quota-warning'),
                text: '空间不够备份整个文件夹。可以取消勾选部分子文件夹，或添加排除规则。',
                icon: LucideIcons.alertTriangle,
                tone: NoteTone.warn,
              )
            else
              const TokensNote(text: '之后在电脑上删除或重命名的文件，会继续保留在云端；修改过的文件会保存为新版本。'),
          ],
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
          onPressed: _submitting ? null : () => Navigator.of(context).pop(),
        ),
        TokensButton(
          key: const Key('backup-start'),
          label: _submitting
              ? '保存中…'
              : _editing
              ? '保存'
              : '开始备份',
          variant: TokensButtonVariant.primary,
          onPressed: _canSubmit ? _submit : null,
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

class _PathRow extends StatelessWidget {
  const _PathRow({
    required this.tokens,
    required this.icon,
    required this.text,
    required this.placeholder,
    required this.buttonLabel,
    required this.onPressed,
    this.mono = false,
    this.buttonKey,
  });

  final DesktopTokens tokens;
  final IconData icon;
  final String? text;
  final String placeholder;
  final String buttonLabel;
  final VoidCallback? onPressed;
  final bool mono;
  final Key? buttonKey;

  @override
  Widget build(BuildContext context) {
    final value = text;
    return Row(
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
                Icon(icon, size: 16, color: tokens.ink400),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    value ?? placeholder,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      color: value == null ? tokens.ink400 : tokens.ink900,
                      fontSize: mono ? tokens.textXs : tokens.textSm,
                      fontFamily: mono ? DesktopMetrics.monoFamily : null,
                      fontFamilyFallback: mono
                          ? DesktopMetrics.monoFallback
                          : null,
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
        const SizedBox(width: 8),
        TokensButton(key: buttonKey, label: buttonLabel, onPressed: onPressed),
      ],
    );
  }
}

class _RuleChip extends StatelessWidget {
  const _RuleChip({
    required this.tokens,
    required this.label,
    required this.onRemove,
    super.key,
  });

  final DesktopTokens tokens;
  final String label;
  final VoidCallback? onRemove;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 26,
      padding: const EdgeInsets.symmetric(horizontal: 10),
      decoration: BoxDecoration(
        color: tokens.canvasStrong,
        borderRadius: BorderRadius.circular(13),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(
            label,
            style: TextStyle(color: tokens.ink600, fontSize: tokens.textXs),
          ),
          const SizedBox(width: 6),
          InkWell(
            key: ValueKey<String>('exclude-rule-remove-$label'),
            onTap: onRemove,
            child: Icon(LucideIcons.x, size: 12, color: tokens.ink500),
          ),
        ],
      ),
    );
  }
}

class _SubfolderRow extends StatelessWidget {
  const _SubfolderRow({
    required this.tokens,
    required this.name,
    required this.included,
    required this.showBorder,
    required this.onChanged,
    super.key,
  });

  final DesktopTokens tokens;
  final String name;
  final bool included;
  final bool showBorder;
  final ValueChanged<bool>? onChanged;

  @override
  Widget build(BuildContext context) {
    return InkWell(
      onTap: onChanged == null ? null : () => onChanged!(!included),
      child: Container(
        height: 36,
        padding: const EdgeInsets.symmetric(horizontal: 12),
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
                color: included ? tokens.brand600 : tokens.surface,
                border: Border.all(
                  color: included ? tokens.brand600 : tokens.lineStrong,
                  width: 1.5,
                ),
                borderRadius: BorderRadius.circular(5),
              ),
              child: included
                  ? Icon(LucideIcons.check, size: 12, color: tokens.onBrand)
                  : null,
            ),
            const SizedBox(width: 10),
            Icon(LucideIcons.folder, size: 16, color: tokens.ink400),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                name,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  color: included ? tokens.ink900 : tokens.ink500,
                  fontSize: tokens.textSm,
                ),
              ),
            ),
            if (!included)
              Text(
                '不备份',
                style: TextStyle(color: tokens.ink500, fontSize: tokens.textXs),
              ),
          ],
        ),
      ),
    );
  }
}

class _PreflightSummary extends StatelessWidget {
  const _PreflightSummary({
    required this.tokens,
    required this.preflight,
    required this.quotaExceeded,
    super.key,
  });

  final DesktopTokens tokens;
  final BackupPreflightDto preflight;
  final bool quotaExceeded;

  @override
  Widget build(BuildContext context) {
    final remaining = preflight.remainingQuotaBytes;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
      decoration: BoxDecoration(
        color: quotaExceeded ? tokens.danger50 : tokens.canvas,
        borderRadius: BorderRadius.circular(tokens.radiusControl),
      ),
      child: Row(
        children: [
          Expanded(
            child: _SummaryCell(
              tokens: tokens,
              value: formatCount(preflight.fileCount),
              label: '个文件',
            ),
          ),
          Expanded(
            child: _SummaryCell(
              tokens: tokens,
              value: formatBytes(preflight.uploadBytes),
              label: '首次需要上传',
              valueColor: quotaExceeded ? tokens.danger700 : null,
            ),
          ),
          Expanded(
            child: _SummaryCell(
              tokens: tokens,
              value: remaining == null ? '—' : formatBytes(remaining),
              label: '剩余云端空间',
            ),
          ),
        ],
      ),
    );
  }
}

class _SummaryCell extends StatelessWidget {
  const _SummaryCell({
    required this.tokens,
    required this.value,
    required this.label,
    this.valueColor,
  });

  final DesktopTokens tokens;
  final String value;
  final String label;
  final Color? valueColor;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          value,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: TextStyle(
            color: valueColor ?? tokens.ink900,
            fontSize: tokens.textLg,
            fontWeight: FontWeight.w600,
          ),
        ),
        const SizedBox(height: 2),
        Text(
          label,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: TextStyle(color: tokens.ink500, fontSize: tokens.textXs),
        ),
      ],
    );
  }
}

class _AddRuleDialog extends StatefulWidget {
  const _AddRuleDialog({required this.existing});

  final List<String> existing;

  @override
  State<_AddRuleDialog> createState() => _AddRuleDialogState();
}

class _AddRuleDialogState extends State<_AddRuleDialog> {
  final TextEditingController _controller = TextEditingController();
  String? _error;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _submit() {
    final rule = _controller.text.trim();
    if (rule.isEmpty) {
      setState(() => _error = '请输入排除规则');
      return;
    }
    if (widget.existing.contains(rule)) {
      setState(() => _error = '这条规则已经存在');
      return;
    }
    Navigator.of(context).pop(rule);
  }

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return TokensDialogShell(
      title: '添加排除规则',
      width: DesktopMetrics.dialogWidthSmall,
      onClose: () => Navigator.of(context).pop(),
      body: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            '支持 glob，例如 *.psd、node_modules、临时导出。',
            style: TextStyle(color: tokens.ink500, fontSize: tokens.textXs),
          ),
          const SizedBox(height: 10),
          AppTextField(
            key: const Key('exclude-rule-input'),
            controller: _controller,
            hint: '例如：*.psd',
            autofocus: true,
            error: _error != null,
            onSubmitted: (_) => _submit(),
          ),
          if (_error != null) ...[
            const SizedBox(height: 6),
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
        TokensButton(label: '取消', onPressed: () => Navigator.of(context).pop()),
        TokensButton(
          key: const Key('exclude-rule-confirm'),
          label: '添加',
          variant: TokensButtonVariant.primary,
          onPressed: _submit,
        ),
      ],
    );
  }
}
