import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/errors/bridge_error_text.dart';
import '../../core/providers/app_providers.dart';
import '../../core/rust/models.dart';
import '../../core/theme/design_tokens.dart';
import '../widgets/app_dialog.dart';
import '../widgets/controls.dart';
import '../widgets/file_icon.dart';

class FolderTarget {
  const FolderTarget({required this.id, required this.name});

  final String id;
  final String name;
}

/// 选择目标文件夹（移动文件、恢复到新位置共用）。
Future<FolderTarget?> showFolderPickerDialog({
  required BuildContext context,
  required String rootId,
  required String title,
  String? excludeNodeId,
  String? currentParentId,
}) {
  return showTokensDialog<FolderTarget>(
    context: context,
    builder: (dialogContext) => _FolderPickerDialog(
      rootId: rootId,
      title: title,
      excludeNodeId: excludeNodeId,
      currentParentId: currentParentId,
    ),
  );
}

class _FolderPickerDialog extends ConsumerStatefulWidget {
  const _FolderPickerDialog({
    required this.rootId,
    required this.title,
    this.excludeNodeId,
    this.currentParentId,
  });

  final String rootId;
  final String title;
  final String? excludeNodeId;
  final String? currentParentId;

  @override
  ConsumerState<_FolderPickerDialog> createState() =>
      _FolderPickerDialogState();
}

class _FolderPickerDialogState extends ConsumerState<_FolderPickerDialog> {
  late String _folderId = widget.rootId;
  late String _folderName = '全部文件';
  final List<FolderTarget> _crumbs = <FolderTarget>[
    const FolderTarget(id: '', name: '全部文件'),
  ];
  List<NodeDto> _folders = const <NodeDto>[];
  bool _loading = true;
  String? _error;

  @override
  void initState() {
    super.initState();
    _crumbs[0] = FolderTarget(id: widget.rootId, name: '全部文件');
    _load(widget.rootId);
  }

  Future<void> _load(String folderId) async {
    setState(() {
      _loading = true;
      _error = null;
    });
    try {
      final nodes = await ref
          .read(nativeBridgeProvider)
          .listFolder(
            folderId,
            const NodeSortDto(field: NodeSortFieldDto.name, descending: false),
          );
      if (!mounted) {
        return;
      }
      setState(() {
        _folders = nodes
            .where((node) => node.kind == NodeKindDto.folder)
            .toList(growable: false);
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

  void _enter(NodeDto folder) {
    setState(() {
      _folderId = folder.id;
      _folderName = folder.name;
      _crumbs.add(FolderTarget(id: folder.id, name: folder.name));
      _folders = const <NodeDto>[];
    });
    _load(folder.id);
  }

  void _goTo(int index) {
    final target = _crumbs[index];
    setState(() {
      _folderId = target.id;
      _folderName = target.name;
      _crumbs.removeRange(index + 1, _crumbs.length);
      _folders = const <NodeDto>[];
    });
    _load(target.id);
  }

  bool get _disabled =>
      _folderId == widget.excludeNodeId || _folderId == widget.currentParentId;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return TokensDialogShell(
      title: widget.title,
      onClose: () => Navigator.of(context).pop(),
      body: SizedBox(
        height: 320,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            SizedBox(
              height: 28,
              child: ListView(
                scrollDirection: Axis.horizontal,
                children: [
                  for (var index = 0; index < _crumbs.length; index += 1)
                    Row(
                      children: [
                        if (index > 0)
                          Icon(
                            LucideIcons.chevronRight,
                            size: 14,
                            color: tokens.ink400,
                          ),
                        TextButton(
                          onPressed: index == _crumbs.length - 1
                              ? null
                              : () => _goTo(index),
                          style: TextButton.styleFrom(
                            padding: EdgeInsets.symmetric(
                              horizontal: tokens.space2,
                            ),
                            minimumSize: Size.zero,
                            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                            foregroundColor: tokens.ink900,
                            disabledForegroundColor: tokens.ink900,
                          ),
                          child: Text(
                            _crumbs[index].name,
                            style: TextStyle(
                              fontSize: tokens.textSm,
                              fontWeight: index == _crumbs.length - 1
                                  ? FontWeight.w600
                                  : FontWeight.w400,
                            ),
                          ),
                        ),
                      ],
                    ),
                ],
              ),
            ),
            SizedBox(height: tokens.space2),
            Expanded(child: _body(tokens)),
          ],
        ),
      ),
      actions: [
        TokensButton(label: '取消', onPressed: () => Navigator.of(context).pop()),
        TokensButton(
          label: '移动到这里',
          variant: TokensButtonVariant.primary,
          onPressed: _disabled
              ? null
              : () =>
                    Navigator.of(context)
                        .pop(FolderTarget(id: _folderId, name: _folderName)),
        ),
      ],
    );
  }

  Widget _body(DesktopTokens tokens) {
    if (_loading) {
      return Center(
        child: SizedBox(
          width: 20,
          height: 20,
          child: CircularProgressIndicator(
            strokeWidth: 2,
            color: tokens.brand600,
          ),
        ),
      );
    }
    if (_error != null) {
      return Center(
        child: Text(
          _error!,
          style: TextStyle(color: tokens.danger700, fontSize: tokens.textSm),
        ),
      );
    }
    if (_folders.isEmpty) {
      return Center(
        child: Text(
          '这个文件夹里没有子文件夹',
          style: TextStyle(color: tokens.ink500, fontSize: tokens.textSm),
        ),
      );
    }
    return ListView.separated(
      itemCount: _folders.length,
      separatorBuilder: (context, index) =>
          Divider(color: tokens.line, height: 1),
      itemBuilder: (context, index) {
        final folder = _folders[index];
        return InkWell(
          onTap: () => _enter(folder),
          child: Padding(
            padding: EdgeInsets.symmetric(
              horizontal: tokens.space2,
              vertical: tokens.space2,
            ),
            child: Row(
              children: [
                const FileIconTile(name: '', folder: true),
                SizedBox(width: tokens.space3),
                Expanded(
                  child: Text(
                    folder.name,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      color: tokens.ink900,
                      fontSize: tokens.textSm,
                    ),
                  ),
                ),
                Icon(LucideIcons.chevronRight, size: 14, color: tokens.ink400),
              ],
            ),
          ),
        );
      },
    );
  }
}
