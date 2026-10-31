import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/clock.dart';
import '../../core/errors/bridge_error_text.dart';
import '../../core/format/format.dart';
import '../../core/platform/file_picker.dart';
import '../../core/providers/app_providers.dart';
import '../../core/rust/models.dart';
import '../../core/theme/design_tokens.dart';
import '../state/file_browser_controller.dart';
import '../state/transfer_controller.dart';
import '../widgets/app_dialog.dart';
import '../widgets/controls.dart';
import '../widgets/file_icon.dart';

Future<void> showVersionsDialog(BuildContext context, {required NodeDto node}) {
  return showTokensDialog<void>(
    context: context,
    builder: (dialogContext) => _VersionsDialog(node: node),
  );
}

class _VersionsDialog extends ConsumerStatefulWidget {
  const _VersionsDialog({required this.node});

  final NodeDto node;

  @override
  ConsumerState<_VersionsDialog> createState() => _VersionsDialogState();
}

class _VersionsDialogState extends ConsumerState<_VersionsDialog> {
  List<VersionDto> _versions = const <VersionDto>[];
  bool _loading = true;
  String? _error;
  String? _busyVersionId;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final versions = await ref
          .read(nativeBridgeProvider)
          .listVersions(widget.node.id);
      if (!mounted) {
        return;
      }
      setState(() {
        _versions = versions;
        _loading = false;
      });
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _error = bridgeErrorText(error);
        _loading = false;
      });
    }
  }

  Future<void> _download(VersionDto version) async {
    final path = await ref
        .read(filePickerProvider)
        .pickSaveLocation(widget.node.name);
    if (path == null || !mounted) {
      return;
    }
    setState(() => _busyVersionId = version.id);
    try {
      await ref
          .read(transferControllerProvider.notifier)
          .startDownload(
            nodeId: widget.node.id,
            versionId: version.id,
            fileName: baseName(path),
            destinationDir: parentPath(path),
          );
      if (mounted) {
        _showMessage('“${widget.node.name}”开始下载');
      }
    } catch (error) {
      if (mounted) {
        _showMessage(bridgeErrorText(error));
      }
    } finally {
      if (mounted) {
        setState(() => _busyVersionId = null);
      }
    }
  }

  Future<void> _restore(VersionDto version) async {
    setState(() => _busyVersionId = version.id);
    try {
      await ref
          .read(fileBrowserProvider.notifier)
          .restoreVersion(widget.node, version.id);
      if (!mounted) {
        return;
      }
      _showMessage('已恢复该版本为当前版本');
      await _load();
    } catch (error) {
      if (mounted) {
        _showMessage(bridgeErrorText(error));
      }
    } finally {
      if (mounted) {
        setState(() => _busyVersionId = null);
      }
    }
  }

  void _showMessage(String message) {
    ScaffoldMessenger.maybeOf(context)?.showSnackBar(
      SnackBar(content: Text(message), behavior: SnackBarBehavior.floating),
    );
  }

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return TokensDialogShell(
      title: '版本历史 · ${widget.node.name}',
      onClose: () => Navigator.of(context).pop(),
      body: _body(tokens),
      actions: [
        TokensButton(label: '关闭', onPressed: () => Navigator.of(context).pop()),
      ],
    );
  }

  Widget _body(DesktopTokens tokens) {
    if (_loading) {
      return Padding(
        padding: EdgeInsets.symmetric(vertical: tokens.space6),
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
      );
    }
    if (_error != null) {
      return Text(
        _error!,
        style: TextStyle(color: tokens.danger700, fontSize: tokens.textSm),
      );
    }
    if (_versions.isEmpty) {
      return Padding(
        padding: EdgeInsets.symmetric(vertical: tokens.space4),
        child: Text(
          '没有可用版本。',
          style: TextStyle(color: tokens.ink600, fontSize: tokens.textSm),
        ),
      );
    }
    return Container(
      decoration: BoxDecoration(
        border: Border.all(color: tokens.line),
        borderRadius: BorderRadius.circular(tokens.radiusControl),
      ),
      child: Column(
        children: [
          for (var index = 0; index < _versions.length; index += 1) ...[
            if (index > 0) Divider(color: tokens.line, height: 1),
            _versionRow(tokens, _versions[index]),
          ],
        ],
      ),
    );
  }

  Widget _versionRow(DesktopTokens tokens, VersionDto version) {
    final busy = _busyVersionId == version.id;
    final now = ref.read(clockProvider)();
    return Padding(
      padding: EdgeInsets.symmetric(
        horizontal: tokens.space4,
        vertical: tokens.space3,
      ),
      child: Row(
        children: [
          const FileIconTile(name: '', folder: false),
          SizedBox(width: tokens.space3),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Flexible(
                      child: Text(
                        formatIsoTime(version.createdAt, now: now),
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          color: tokens.ink900,
                          fontSize: tokens.textSm,
                          fontWeight: FontWeight.w500,
                        ),
                      ),
                    ),
                    if (version.isCurrent) ...[
                      SizedBox(width: tokens.space2),
                      const TokensPill(label: '当前版本', tone: PillTone.ok),
                    ],
                  ],
                ),
                SizedBox(height: tokens.space1),
                Text(
                  formatBytes(version.size),
                  style: TextStyle(
                    color: tokens.ink600,
                    fontSize: tokens.textXs,
                  ),
                ),
              ],
            ),
          ),
          TokensButton(
            key: ValueKey<String>('download-version-${version.id}'),
            label: '下载',
            icon: LucideIcons.download,
            onPressed: busy ? null : () => _download(version),
          ),
          if (!version.isCurrent) ...[
            SizedBox(width: tokens.space2),
            TokensButton(
              key: ValueKey<String>('restore-version-${version.id}'),
              label: '恢复此版本',
              icon: LucideIcons.rotateCcw,
              onPressed: busy ? null : () => _restore(version),
            ),
          ],
        ],
      ),
    );
  }
}
