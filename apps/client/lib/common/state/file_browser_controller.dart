import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/errors/bridge_error_text.dart';
import '../../core/providers/app_providers.dart';
import '../../core/rust/models.dart';
import '../../core/theme/design_tokens.dart';
import 'auth_controller.dart';

enum FileViewMode { list, grid }

@immutable
class FolderCrumb {
  const FolderCrumb({required this.id, required this.name});

  final String id;
  final String name;
}

@immutable
class FileBrowserState {
  const FileBrowserState({
    required this.rootId,
    required this.crumbs,
    this.items = const <NodeDto>[],
    this.loading = true,
    this.error,
    this.mutating = false,
    this.visibleCount = DesktopMetrics.filePageSize,
    this.sort = const NodeSortDto(
      field: NodeSortFieldDto.name,
      descending: false,
    ),
    this.viewMode = FileViewMode.list,
    this.filter = '',
    this.selectedId,
  });

  final String? rootId;
  final List<FolderCrumb> crumbs;
  final List<NodeDto> items;
  final bool loading;
  final String? error;
  final bool mutating;
  final int visibleCount;
  final NodeSortDto sort;
  final FileViewMode viewMode;
  final String filter;
  final String? selectedId;

  String? get currentFolderId => crumbs.isEmpty ? rootId : crumbs.last.id;

  String get currentFolderName => crumbs.isEmpty ? '全部文件' : crumbs.last.name;

  List<NodeDto> get filteredItems {
    final needle = filter.trim().toLowerCase();
    if (needle.isEmpty) {
      return items;
    }
    return items
        .where((item) => item.name.toLowerCase().contains(needle))
        .toList(growable: false);
  }

  List<NodeDto> get visibleItems =>
      filteredItems.take(visibleCount).toList(growable: false);

  bool get hasMore => filteredItems.length > visibleCount;

  bool get isFiltering => filter.trim().isNotEmpty;

  FileBrowserState copyWith({
    String? rootId,
    List<FolderCrumb>? crumbs,
    List<NodeDto>? items,
    bool? loading,
    String? error,
    bool clearError = false,
    bool? mutating,
    int? visibleCount,
    NodeSortDto? sort,
    FileViewMode? viewMode,
    String? filter,
    String? selectedId,
    bool clearSelection = false,
  }) {
    return FileBrowserState(
      rootId: rootId ?? this.rootId,
      crumbs: crumbs ?? this.crumbs,
      items: items ?? this.items,
      loading: loading ?? this.loading,
      error: clearError ? null : (error ?? this.error),
      mutating: mutating ?? this.mutating,
      visibleCount: visibleCount ?? this.visibleCount,
      sort: sort ?? this.sort,
      viewMode: viewMode ?? this.viewMode,
      filter: filter ?? this.filter,
      selectedId: clearSelection ? null : (selectedId ?? this.selectedId),
    );
  }
}

/// 当前文件夹的浏览状态：面包屑、排序、筛选、本地分页与写操作。
///
/// 桥接 `list_folder` 一次返回当前文件夹的全部子节点（client-core 已聚合
/// 服务端分页）；界面按 [DesktopMetrics.filePageSize] 分页展示。
class FileBrowserController extends Notifier<FileBrowserState> {
  @override
  FileBrowserState build() {
    final account = ref.read(authControllerProvider).account;
    final rootId = account?.rootNodeId;
    final initial = FileBrowserState(
      rootId: rootId,
      crumbs: rootId == null
          ? const <FolderCrumb>[]
          : <FolderCrumb>[FolderCrumb(id: rootId, name: '全部文件')],
      loading: rootId != null,
      error: rootId == null ? '还没有可浏览的账号' : null,
    );
    ref.listen(authControllerProvider, (previous, next) {
      final nextRoot = next.account?.rootNodeId;
      if (nextRoot != null && nextRoot != state.rootId) {
        state = FileBrowserState(
          rootId: nextRoot,
          crumbs: <FolderCrumb>[FolderCrumb(id: nextRoot, name: '全部文件')],
        );
        Future<void>.microtask(() => _load(nextRoot));
      }
    });
    if (rootId != null) {
      Future<void>.microtask(() => _load(rootId));
    }
    return initial;
  }

  bool get canWrite {
    final account = ref.read(authControllerProvider).account;
    return account != null && account.writesAllowed && account.unlocked;
  }

  /// 加载指定文件夹；`reset` 为 false 时只刷新数据，不改面包屑。
  Future<void> _load(String folderId) async {
    state = state.copyWith(loading: true, clearError: true);
    try {
      final items = await ref
          .read(nativeBridgeProvider)
          .listFolder(folderId, state.sort);
      if (!ref.mounted || state.currentFolderId != folderId) {
        return;
      }
      state = state.copyWith(
        items: items,
        loading: false,
        visibleCount: DesktopMetrics.filePageSize,
        clearError: true,
      );
    } catch (error) {
      if (!ref.mounted || state.currentFolderId != folderId) {
        return;
      }
      state = state.copyWith(loading: false, error: bridgeErrorText(error));
    }
  }

  Future<void> refresh() async {
    final folderId = state.currentFolderId;
    if (folderId != null) {
      await _load(folderId);
    }
  }

  void loadMore() {
    if (!state.hasMore) {
      return;
    }
    state = state.copyWith(
      visibleCount: state.visibleCount + DesktopMetrics.filePageSize,
    );
  }

  Future<void> setSort(NodeSortFieldDto field) async {
    final same = state.sort.field == field;
    final next = NodeSortDto(
      field: field,
      descending: same ? !state.sort.descending : false,
    );
    state = state.copyWith(sort: next);
    final folderId = state.currentFolderId;
    if (folderId != null) {
      await _load(folderId);
    }
  }

  void setViewMode(FileViewMode mode) {
    state = state.copyWith(viewMode: mode);
  }

  void setFilter(String value) {
    state = state.copyWith(
      filter: value,
      visibleCount: DesktopMetrics.filePageSize,
    );
  }

  void select(String? nodeId) {
    state = nodeId == null
        ? state.copyWith(clearSelection: true)
        : state.copyWith(selectedId: nodeId);
  }

  Future<void> openFolder(NodeDto folder) async {
    if (folder.kind != NodeKindDto.folder) {
      return;
    }
    final crumbs = <FolderCrumb>[
      ...state.crumbs,
      FolderCrumb(id: folder.id, name: folder.name),
    ];
    state = state.copyWith(
      crumbs: crumbs,
      items: const <NodeDto>[],
      loading: true,
      filter: '',
      clearSelection: true,
      clearError: true,
    );
    await _load(folder.id);
  }

  Future<void> goToCrumb(int index) async {
    if (index < 0 || index >= state.crumbs.length) {
      return;
    }
    final crumbs = state.crumbs.sublist(0, index + 1);
    final target = crumbs.last;
    state = state.copyWith(
      crumbs: crumbs,
      items: const <NodeDto>[],
      loading: true,
      filter: '',
      clearSelection: true,
      clearError: true,
    );
    await _load(target.id);
  }

  Future<void> goUp() async {
    if (state.crumbs.length > 1) {
      await goToCrumb(state.crumbs.length - 2);
    }
  }

  Future<NodeDto> createFolder(String name) async {
    final parent = state.currentFolderId;
    if (parent == null) {
      throw StateError('当前文件夹不可用');
    }
    return _mutate(() async {
      final node = await ref
          .read(nativeBridgeProvider)
          .createFolder(parent, name);
      await _reloadAfterMutation();
      return node;
    });
  }

  Future<NodeDto> rename(NodeDto node, String newName) async {
    final parent = node.parentId ?? state.currentFolderId;
    if (parent == null) {
      throw StateError('当前文件夹不可用');
    }
    return _mutate(() async {
      final updated = await ref
          .read(nativeBridgeProvider)
          .renameNode(
            nodeId: node.id,
            parentId: parent,
            newName: newName,
            rowVersion: node.rowVersion,
          );
      await _reloadAfterMutation();
      return updated;
    });
  }

  Future<NodeDto> move(NodeDto node, String targetId) async {
    final parent = node.parentId ?? state.currentFolderId;
    if (parent == null) {
      throw StateError('当前文件夹不可用');
    }
    return _mutate(() async {
      final updated = await ref
          .read(nativeBridgeProvider)
          .moveNode(
            nodeId: node.id,
            currentParentId: parent,
            newParentId: targetId,
            name: node.name,
            rowVersion: node.rowVersion,
          );
      await _reloadAfterMutation();
      return updated;
    });
  }

  Future<void> deleteToTrash(NodeDto node) async {
    await _mutate(() async {
      await ref.read(nativeBridgeProvider).deleteToTrash(node.id);
      await _reloadAfterMutation();
    });
  }

  Future<VersionDto> restoreVersion(NodeDto node, String versionId) async {
    return _mutate(() async {
      final version = await ref
          .read(nativeBridgeProvider)
          .restoreVersion(node.id, versionId);
      await _reloadAfterMutation();
      return version;
    });
  }

  Future<T> _mutate<T>(Future<T> Function() action) async {
    if (!canWrite) {
      throw const BridgeError(
        code: 'account_not_writable',
        message: '账号通过审核后才能上传和备份文件',
      );
    }
    state = state.copyWith(mutating: true, clearError: true);
    try {
      return await action();
    } finally {
      if (ref.mounted) {
        state = state.copyWith(mutating: false);
      }
    }
  }

  Future<void> _reloadAfterMutation() async {
    ref.invalidate(usageProvider);
    final folderId = state.currentFolderId;
    if (folderId != null) {
      await _load(folderId);
    }
  }
}

final fileBrowserProvider =
    NotifierProvider<FileBrowserController, FileBrowserState>(
      FileBrowserController.new,
    );
