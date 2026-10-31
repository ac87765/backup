import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/clock.dart';
import '../../core/errors/bridge_error_text.dart';
import '../../core/format/format.dart';
import '../../core/platform/file_picker.dart';
import '../../core/rust/models.dart';
import '../../core/theme/design_tokens.dart';
import '../dialogs/folder_picker_dialog.dart';
import '../dialogs/name_dialog.dart';
import '../dialogs/versions_dialog.dart';
import '../state/file_browser_controller.dart';
import '../state/transfer_controller.dart';
import '../widgets/controls.dart';
import '../widgets/file_icon.dart';
import '../widgets/popover_menu.dart';

/// 全部文件（files.html 的 default / menu 状态）。
class FilesPage extends ConsumerStatefulWidget {
  const FilesPage({super.key});

  @override
  ConsumerState<FilesPage> createState() => _FilesPageState();
}

class _FilesPageState extends ConsumerState<FilesPage> {
  final TextEditingController _filter = TextEditingController();

  @override
  void dispose() {
    _filter.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final state = ref.watch(fileBrowserProvider);
    if (_filter.text != state.filter) {
      // 清除筛选等外部修改后同步输入框；放到帧后避免在 build 期间触发
      // TextField 的 setState。
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted || _filter.text == state.filter) {
          return;
        }
        _filter.value = TextEditingValue(
          text: state.filter,
          selection: TextSelection.collapsed(offset: state.filter.length),
        );
      });
    }
    return Column(
      children: [
        _Toolbar(
          state: state,
          filterController: _filter,
          onUpload: _uploadFiles,
          onCreateFolder: _createFolder,
          onRefresh: () => ref.read(fileBrowserProvider.notifier).refresh(),
        ),
        Expanded(child: _content(state)),
      ],
    );
  }

  Widget _content(FileBrowserState state) {
    final tokens = DesktopTokens.of(context);
    if (state.loading && state.items.isEmpty) {
      return Center(
        child: SizedBox(
          width: 22,
          height: 22,
          child: CircularProgressIndicator(
            strokeWidth: 2,
            color: tokens.brand600,
          ),
        ),
      );
    }
    if (state.error != null && state.items.isEmpty) {
      return _ErrorState(
        message: state.error!,
        onRetry: () => ref.read(fileBrowserProvider.notifier).refresh(),
      );
    }
    if (state.visibleItems.isEmpty) {
      return _EmptyState(
        filtering: state.isFiltering,
        canWrite: ref.read(fileBrowserProvider.notifier).canWrite,
        filter: state.filter,
        onClearFilter: () {
          ref.read(fileBrowserProvider.notifier).setFilter('');
        },
        onUpload: _uploadFiles,
        onCreateFolder: _createFolder,
      );
    }
    final list = state.viewMode == FileViewMode.list
        ? _listView(state)
        : _gridView(state);
    return Column(
      children: [
        if (state.loading) LinearProgressIndicator(minHeight: 2),
        Expanded(child: list),
        if (state.hasMore)
          _LoadMore(
            shown: state.visibleItems.length,
            total: state.filteredItems.length,
            onPressed: () => ref.read(fileBrowserProvider.notifier).loadMore(),
          ),
      ],
    );
  }

  Widget _listView(FileBrowserState state) {
    final now = ref.read(clockProvider)();
    return Column(
      children: [
        _TableHeader(state: state),
        Expanded(
          child: ListView.builder(
            itemCount: state.visibleItems.length,
            itemBuilder: (context, index) {
              final node = state.visibleItems[index];
              return _FileRow(
                key: ValueKey<String>('file-row-${node.id}'),
                node: node,
                now: now,
                selected: state.selectedId == node.id,
                onTap: () =>
                    ref.read(fileBrowserProvider.notifier).select(node.id),
                onOpen: () => _openNode(node),
                onMore: (rowContext) => _openMenu(rowContext, node),
              );
            },
          ),
        ),
      ],
    );
  }

  Widget _gridView(FileBrowserState state) {
    return GridView.builder(
      key: const Key('file-grid'),
      padding: const EdgeInsets.all(16),
      gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(
        maxCrossAxisExtent: 200,
        mainAxisExtent: 154,
        crossAxisSpacing: 12,
        mainAxisSpacing: 12,
      ),
      itemCount: state.visibleItems.length,
      itemBuilder: (context, index) {
        final node = state.visibleItems[index];
        return _FileCard(
          key: ValueKey<String>('file-card-${node.id}'),
          node: node,
          selected: state.selectedId == node.id,
          onTap: () => ref.read(fileBrowserProvider.notifier).select(node.id),
          onOpen: () => _openNode(node),
          onMore: (cardContext) => _openMenu(cardContext, node),
        );
      },
    );
  }

  Future<void> _openNode(NodeDto node) async {
    if (node.kind == NodeKindDto.folder) {
      await ref.read(fileBrowserProvider.notifier).openFolder(node);
    } else {
      await _download(node);
    }
  }

  Future<void> _openMenu(BuildContext rowContext, NodeDto node) async {
    final canWrite = ref.read(fileBrowserProvider.notifier).canWrite;
    final folder = node.kind == NodeKindDto.folder;
    final anchor = anchorOf(rowContext);
    await showTokensMenu(
      context: context,
      anchor: anchor,
      items: [
        if (!folder)
          TokensMenuItem(
            label: '下载',
            icon: LucideIcons.download,
            onSelected: () => _download(node),
          ),
        if (folder)
          TokensMenuItem(
            label: '打开',
            icon: LucideIcons.folderOpen,
            onSelected: () => _openNode(node),
          ),
        if (!folder)
          TokensMenuItem(
            label: '上传新版本',
            icon: LucideIcons.upload,
            onSelected: canWrite
                ? () => _uploadNewVersion(node)
                : () => _showMessage('账号通过审核后才能上传和备份文件'),
          ),
        if (!folder)
          TokensMenuItem(
            label: '版本历史',
            icon: LucideIcons.history,
            highlighted: true,
            onSelected: () => showVersionsDialog(context, node: node),
          ),
        TokensMenuItem(
          label: '重命名',
          icon: LucideIcons.file,
          separatorBefore: true,
          onSelected: canWrite
              ? () => _rename(node)
              : () => _showMessage('账号通过审核后才能上传和备份文件'),
        ),
        TokensMenuItem(
          label: '移动到…',
          icon: LucideIcons.arrowRight,
          onSelected: canWrite
              ? () => _move(node)
              : () => _showMessage('账号通过审核后才能上传和备份文件'),
        ),
        TokensMenuItem(
          label: '移到回收站',
          icon: LucideIcons.trash,
          danger: true,
          separatorBefore: true,
          onSelected: canWrite
              ? () => _delete(node)
              : () => _showMessage('账号通过审核后才能上传和备份文件'),
        ),
      ],
    );
  }

  Future<void> _createFolder() async {
    final name = await showNodeNameDialog(
      context: context,
      title: '新建文件夹',
      label: '文件夹名称',
      submitLabel: '创建',
      placeholder: '例如：合同',
      onSubmit: (value) async {
        try {
          await ref.read(fileBrowserProvider.notifier).createFolder(value);
          return null;
        } catch (error) {
          return bridgeErrorText(error);
        }
      },
    );
    if (name != null && mounted) {
      _showMessage('文件夹已创建');
    }
  }

  Future<void> _rename(NodeDto node) async {
    final name = await showNodeNameDialog(
      context: context,
      title: '重命名',
      label: '新名称',
      submitLabel: '保存',
      initialValue: node.name,
      onSubmit: (value) async {
        try {
          await ref.read(fileBrowserProvider.notifier).rename(node, value);
          return null;
        } catch (error) {
          return bridgeErrorText(error);
        }
      },
    );
    if (name != null && mounted) {
      _showMessage('已重命名');
    }
  }

  Future<void> _move(NodeDto node) async {
    final state = ref.read(fileBrowserProvider);
    final rootId = state.rootId;
    if (rootId == null) {
      return;
    }
    final target = await showFolderPickerDialog(
      context: context,
      rootId: rootId,
      title: '移动“${node.name}”',
      excludeNodeId: node.id,
      currentParentId: node.parentId ?? state.currentFolderId,
    );
    if (target == null || !mounted) {
      return;
    }
    try {
      await ref.read(fileBrowserProvider.notifier).move(node, target.id);
      if (mounted) {
        _showMessage('已移动到“${target.name}”');
      }
    } catch (error) {
      if (mounted) {
        _showMessage(bridgeErrorText(error));
      }
    }
  }

  Future<void> _delete(NodeDto node) async {
    try {
      await ref.read(fileBrowserProvider.notifier).deleteToTrash(node);
      if (mounted) {
        _showMessage('“${node.name}”已移入回收站');
      }
    } catch (error) {
      if (mounted) {
        _showMessage(bridgeErrorText(error));
      }
    }
  }

  Future<void> _download(NodeDto node) async {
    final versionId = node.currentVersionId;
    if (versionId == null) {
      _showMessage('该文件没有可下载的版本');
      return;
    }
    final path = await ref.read(filePickerProvider).pickSaveLocation(node.name);
    if (path == null || !mounted) {
      return;
    }
    try {
      await ref
          .read(transferControllerProvider.notifier)
          .startDownload(
            nodeId: node.id,
            versionId: versionId,
            fileName: baseName(path),
            destinationDir: parentPath(path),
          );
      if (mounted) {
        _showMessage('“${baseName(path)}”开始下载');
      }
    } catch (error) {
      if (mounted) {
        _showMessage(bridgeErrorText(error));
      }
    }
  }

  Future<void> _uploadFiles() async {
    if (!ref.read(fileBrowserProvider.notifier).canWrite) {
      _showMessage('账号通过审核后才能上传和备份文件');
      return;
    }
    final parent = ref.read(fileBrowserProvider).currentFolderId;
    if (parent == null) {
      return;
    }
    final paths = await ref.read(filePickerProvider).pickFiles();
    if (paths.isEmpty || !mounted) {
      return;
    }
    try {
      await ref
          .read(transferControllerProvider.notifier)
          .startUploadFiles(parent, paths);
      if (mounted) {
        _showMessage('已开始上传 ${paths.length} 个文件');
      }
    } catch (error) {
      if (mounted) {
        _showMessage(bridgeErrorText(error));
      }
    }
  }

  Future<void> _uploadNewVersion(NodeDto node) async {
    final paths = await ref.read(filePickerProvider).pickFiles();
    if (paths.isEmpty || !mounted) {
      return;
    }
    try {
      await ref
          .read(transferControllerProvider.notifier)
          .startUploadNewVersion(node.id, paths.first);
      if (mounted) {
        _showMessage('已开始上传新版本');
      }
    } catch (error) {
      if (mounted) {
        _showMessage(bridgeErrorText(error));
      }
    }
  }

  void _showMessage(String message) {
    ScaffoldMessenger.maybeOf(context)?.showSnackBar(
      SnackBar(
        content: Text(message),
        behavior: SnackBarBehavior.floating,
        width: 360,
      ),
    );
  }
}

class _Toolbar extends ConsumerWidget {
  const _Toolbar({
    required this.state,
    required this.filterController,
    required this.onUpload,
    required this.onCreateFolder,
    required this.onRefresh,
  });

  final FileBrowserState state;
  final TextEditingController filterController;
  final VoidCallback onUpload;
  final VoidCallback onCreateFolder;
  final VoidCallback onRefresh;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = DesktopTokens.of(context);
    final canWrite = ref.read(fileBrowserProvider.notifier).canWrite;
    return Container(
      height: DesktopMetrics.toolbarHeight,
      padding: const EdgeInsets.symmetric(horizontal: 20),
      decoration: BoxDecoration(
        border: Border(bottom: BorderSide(color: tokens.line)),
      ),
      child: Row(
        children: [
          Expanded(child: _Breadcrumbs(state: state)),
          const SizedBox(width: 8),
          SizedBox(
            width: 240,
            child: AppTextField(
              key: const Key('file-filter'),
              controller: filterController,
              small: true,
              hint: '筛选当前文件夹',
              prefixIcon: LucideIcons.search,
              onChanged: (value) =>
                  ref.read(fileBrowserProvider.notifier).setFilter(value),
            ),
          ),
          const SizedBox(width: 8),
          TokensSegmented<FileViewMode>(
            key: const Key('view-toggle'),
            value: state.viewMode,
            onChanged: (value) =>
                ref.read(fileBrowserProvider.notifier).setViewMode(value),
            options: const [
              SegmentedOption(
                value: FileViewMode.list,
                icon: LucideIcons.list,
                tooltip: '列表视图',
              ),
              SegmentedOption(
                value: FileViewMode.grid,
                icon: LucideIcons.grid,
                tooltip: '网格视图',
              ),
            ],
          ),
          const SizedBox(width: 8),
          TokensIconButton(
            icon: LucideIcons.refreshCw,
            tooltip: '刷新',
            onPressed: onRefresh,
          ),
          const SizedBox(width: 8),
          Tooltip(
            message: canWrite ? '新建文件夹' : '账号通过审核后才能新建文件夹',
            child: TokensButton(
              label: '新建文件夹',
              icon: LucideIcons.folderPlus,
              onPressed: canWrite ? onCreateFolder : null,
            ),
          ),
          const SizedBox(width: 8),
          _UploadButton(canWrite: canWrite, onUpload: onUpload),
        ],
      ),
    );
  }
}

class _UploadButton extends StatelessWidget {
  const _UploadButton({required this.canWrite, required this.onUpload});

  final bool canWrite;
  final VoidCallback onUpload;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: canWrite ? '上传文件' : '账号通过审核后才能上传',
      child: TokensButton(
        label: '上传',
        icon: LucideIcons.upload,
        trailingIcon: LucideIcons.chevronDown,
        variant: TokensButtonVariant.primary,
        onPressed: canWrite ? onUpload : null,
      ),
    );
  }
}

class _Breadcrumbs extends ConsumerWidget {
  const _Breadcrumbs({required this.state});

  final FileBrowserState state;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = DesktopTokens.of(context);
    return SingleChildScrollView(
      scrollDirection: Axis.horizontal,
      child: Row(
        children: [
          for (var index = 0; index < state.crumbs.length; index += 1) ...[
            if (index > 0)
              Padding(
                padding: const EdgeInsets.symmetric(horizontal: 4),
                child: Icon(
                  LucideIcons.chevronRight,
                  size: 16,
                  color: tokens.ink400,
                ),
              ),
            InkWell(
              onTap: index == state.crumbs.length - 1
                  ? null
                  : () =>
                        ref.read(fileBrowserProvider.notifier).goToCrumb(index),
              borderRadius: BorderRadius.circular(4),
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 2, vertical: 2),
                child: Text(
                  state.crumbs[index].name,
                  style: TextStyle(
                    color: index == state.crumbs.length - 1
                        ? tokens.ink900
                        : tokens.ink500,
                    fontSize: tokens.textLg,
                    fontWeight: index == state.crumbs.length - 1
                        ? FontWeight.w600
                        : FontWeight.w400,
                  ),
                ),
              ),
            ),
          ],
        ],
      ),
    );
  }
}

class _TableHeader extends ConsumerWidget {
  const _TableHeader({required this.state});

  final FileBrowserState state;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = DesktopTokens.of(context);
    Widget header(String label, NodeSortFieldDto field) {
      final active = state.sort.field == field;
      return InkWell(
        onTap: () => ref.read(fileBrowserProvider.notifier).setSort(field),
        child: Row(
          children: [
            Text(
              label,
              style: TextStyle(
                color: active ? tokens.ink900 : tokens.ink500,
                fontSize: tokens.textXs,
                fontWeight: FontWeight.w500,
              ),
            ),
            if (active) ...[
              const SizedBox(width: 2),
              Icon(
                state.sort.descending
                    ? LucideIcons.chevronUp
                    : LucideIcons.chevronDown,
                size: 12,
                color: tokens.ink500,
              ),
            ],
          ],
        ),
      );
    }

    return Container(
      height: DesktopMetrics.tableHeaderHeight,
      decoration: BoxDecoration(
        border: Border(bottom: BorderSide(color: tokens.line)),
      ),
      child: Row(
        children: [
          Expanded(
            child: Padding(
              padding: const EdgeInsets.only(
                left: DesktopMetrics.tableNameColumnPadding,
              ),
              child: header('名称', NodeSortFieldDto.name),
            ),
          ),
          SizedBox(
            width: DesktopMetrics.tableModifiedWidth,
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12),
              child: header('修改时间', NodeSortFieldDto.modifiedTime),
            ),
          ),
          SizedBox(
            width: DesktopMetrics.tableSizeWidth,
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 12),
              child: header('大小', NodeSortFieldDto.size),
            ),
          ),
          const SizedBox(width: DesktopMetrics.tableActionsWidth),
        ],
      ),
    );
  }
}

class _FileRow extends StatefulWidget {
  const _FileRow({
    required this.node,
    required this.now,
    required this.selected,
    required this.onTap,
    required this.onOpen,
    required this.onMore,
    super.key,
  });

  final NodeDto node;
  final DateTime now;
  final bool selected;
  final VoidCallback onTap;
  final VoidCallback onOpen;
  final ValueChanged<BuildContext> onMore;

  @override
  State<_FileRow> createState() => _FileRowState();
}

class _FileRowState extends State<_FileRow> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final node = widget.node;
    final background = widget.selected
        ? tokens.brand50
        : _hover
        ? tokens.canvas
        : tokens.surface;
    final showActions = _hover || widget.selected;
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        onTap: widget.onTap,
        onDoubleTap: widget.onOpen,
        child: Container(
          height: DesktopMetrics.tableRowHeight,
          decoration: BoxDecoration(
            color: background,
            border: Border(bottom: BorderSide(color: tokens.line)),
          ),
          child: Row(
            children: [
              Expanded(
                child: Padding(
                  padding: const EdgeInsets.only(
                    left: DesktopMetrics.tableNameColumnPadding,
                    right: 12,
                  ),
                  child: Row(
                    children: [
                      FileIconTile.node(node),
                      const SizedBox(width: 12),
                      Expanded(
                        child: Text(
                          node.name,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            color: tokens.ink900,
                            fontSize: tokens.textSm,
                            fontWeight: FontWeight.w500,
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
              SizedBox(
                width: DesktopMetrics.tableModifiedWidth,
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 12),
                  child: Text(
                    _modifiedText(node),
                    style: TextStyle(
                      color: tokens.ink500,
                      fontSize: tokens.textSm,
                    ),
                  ),
                ),
              ),
              SizedBox(
                width: DesktopMetrics.tableSizeWidth,
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 12),
                  child: Text(
                    node.kind == NodeKindDto.folder
                        ? '—'
                        : formatBytes(node.size),
                    style: TextStyle(
                      color: tokens.ink500,
                      fontSize: tokens.textSm,
                    ),
                  ),
                ),
              ),
              SizedBox(
                width: DesktopMetrics.tableActionsWidth,
                child: showActions
                    ? Align(
                        alignment: Alignment.centerRight,
                        child: Padding(
                          padding: const EdgeInsets.only(right: 8),
                          child: Builder(
                            builder: (buttonContext) => TokensIconButton(
                              key: ValueKey<String>('file-more-${node.id}'),
                              icon: LucideIcons.moreHorizontal,
                              tooltip: '更多操作',
                              onPressed: () => widget.onMore(buttonContext),
                            ),
                          ),
                        ),
                      )
                    : null,
              ),
            ],
          ),
        ),
      ),
    );
  }

  String _modifiedText(NodeDto node) {
    if (node.mtime != null) {
      return formatUnixTime(node.mtime, now: widget.now);
    }
    return formatIsoTime(node.updatedAt, now: widget.now);
  }
}

class _FileCard extends StatefulWidget {
  const _FileCard({
    required this.node,
    required this.selected,
    required this.onTap,
    required this.onOpen,
    required this.onMore,
    super.key,
  });

  final NodeDto node;
  final bool selected;
  final VoidCallback onTap;
  final VoidCallback onOpen;
  final ValueChanged<BuildContext> onMore;

  @override
  State<_FileCard> createState() => _FileCardState();
}

class _FileCardState extends State<_FileCard> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final node = widget.node;
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        onTap: widget.onTap,
        onDoubleTap: widget.onOpen,
        child: Container(
          padding: const EdgeInsets.fromLTRB(12, 16, 12, 12),
          decoration: BoxDecoration(
            color: widget.selected
                ? tokens.brand50
                : _hover
                ? tokens.canvas
                : tokens.surface,
            border: Border.all(
              color: widget.selected ? tokens.brand600 : tokens.line,
            ),
            borderRadius: BorderRadius.circular(tokens.radiusCard),
          ),
          child: Stack(
            children: [
              Column(
                children: [
                  FileIconTile.node(node, size: 56, iconSize: 28),
                  const SizedBox(height: 12),
                  Expanded(
                    child: Text(
                      node.name,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      textAlign: TextAlign.center,
                      style: TextStyle(
                        color: tokens.ink900,
                        fontSize: tokens.textSm,
                        fontWeight: FontWeight.w500,
                      ),
                    ),
                  ),
                  Text(
                    node.kind == NodeKindDto.folder
                        ? '文件夹'
                        : formatBytes(node.size),
                    style: TextStyle(
                      color: tokens.ink600,
                      fontSize: tokens.textXs,
                    ),
                  ),
                ],
              ),
              if (_hover || widget.selected)
                Positioned(
                  right: -4,
                  top: -8,
                  child: Builder(
                    builder: (buttonContext) => TokensIconButton(
                      key: ValueKey<String>('file-card-more-${node.id}'),
                      icon: LucideIcons.moreHorizontal,
                      tooltip: '更多操作',
                      onPressed: () => widget.onMore(buttonContext),
                    ),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}

class _EmptyState extends StatelessWidget {
  const _EmptyState({
    required this.filtering,
    required this.canWrite,
    required this.filter,
    required this.onClearFilter,
    required this.onUpload,
    required this.onCreateFolder,
  });

  final bool filtering;
  final bool canWrite;
  final String filter;
  final VoidCallback onClearFilter;
  final VoidCallback onUpload;
  final VoidCallback onCreateFolder;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Center(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 420),
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Container(
                width: 56,
                height: 56,
                decoration: BoxDecoration(
                  color: tokens.canvas,
                  shape: BoxShape.circle,
                ),
                child: Icon(
                  filtering ? LucideIcons.search : LucideIcons.folderOpen,
                  size: 26,
                  color: tokens.ink500,
                ),
              ),
              const SizedBox(height: 16),
              Text(
                filtering ? '没有匹配的文件' : '这个文件夹还是空的',
                style: TextStyle(
                  color: tokens.ink900,
                  fontSize: tokens.textLg,
                  fontWeight: FontWeight.w600,
                ),
              ),
              const SizedBox(height: 6),
              Text(
                filtering
                    ? '当前文件夹中没有名称包含“${filter.trim()}”的项目。'
                    : '上传文件，或先新建一个文件夹。',
                textAlign: TextAlign.center,
                style: TextStyle(color: tokens.ink500, fontSize: tokens.textSm),
              ),
              const SizedBox(height: 20),
              if (filtering)
                TokensButton(label: '清除筛选', onPressed: onClearFilter)
              else
                Row(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    TokensButton(
                      label: '上传文件',
                      icon: LucideIcons.upload,
                      variant: TokensButtonVariant.primary,
                      onPressed: canWrite ? onUpload : null,
                    ),
                    const SizedBox(width: 8),
                    TokensButton(
                      label: '新建文件夹',
                      icon: LucideIcons.folderPlus,
                      onPressed: canWrite ? onCreateFolder : null,
                    ),
                  ],
                ),
            ],
          ),
        ),
      ),
    );
  }
}

class _ErrorState extends StatelessWidget {
  const _ErrorState({required this.message, required this.onRetry});

  final String message;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(LucideIcons.alertTriangle, size: 28, color: tokens.danger600),
          const SizedBox(height: 12),
          Text(
            message,
            textAlign: TextAlign.center,
            style: TextStyle(color: tokens.ink600, fontSize: tokens.textSm),
          ),
          const SizedBox(height: 16),
          TokensButton(label: '重试', onPressed: onRetry),
        ],
      ),
    );
  }
}

class _LoadMore extends StatelessWidget {
  const _LoadMore({
    required this.shown,
    required this.total,
    required this.onPressed,
  });

  final int shown;
  final int total;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Container(
      padding: const EdgeInsets.symmetric(vertical: 8),
      decoration: BoxDecoration(
        border: Border(top: BorderSide(color: tokens.line)),
      ),
      child: Center(
        child: TokensButton(
          key: const Key('load-more-files'),
          label: '加载更多（已显示 $shown / $total）',
          onPressed: onPressed,
        ),
      ),
    );
  }
}
