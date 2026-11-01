import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../common/state/auth_controller.dart';
import '../../core/errors/bridge_error_text.dart';
import '../../core/providers/app_providers.dart';
import '../../core/rust/models.dart';
import '../../core/rust/native_bridge.dart';
import 'wechat_paths.dart';

/// 备份文件夹页的展示状态：文件夹配置 + 引擎运行状态 + 最近活动 + 汇总。
@immutable
class BackupFoldersState {
  const BackupFoldersState({
    this.loading = false,
    this.error,
    this.folders = const <BackupFolderDto>[],
    this.folderStates = const <String, BackupFolderStateDto>{},
    this.activities = const <BackupActivityDto>[],
    this.summary,
    this.wechatDataRoots = const <String>[],
    this.wechatExcludedBytes = const <String, int>{},
  });

  final bool loading;
  final String? error;
  final List<BackupFolderDto> folders;
  final Map<String, BackupFolderStateDto> folderStates;
  final List<BackupActivityDto> activities;
  final BackupSummaryDto? summary;

  /// D1 发现的微信数据根（用于判断普通文件夹是否位于微信目录内）。
  final List<String> wechatDataRoots;

  /// 普通备份文件夹预检时被自动排除的微信数据字节数（按 folder id）。
  final Map<String, int> wechatExcludedBytes;

  bool get isEmpty => !loading && folders.isEmpty && error == null;

  bool get globalPaused => summary?.kind == BackupSummaryStatusKindDto.paused;

  BackupFolderStateDto? stateFor(String folderId) => folderStates[folderId];

  bool managedByWechat(BackupFolderDto folder) =>
      isPathInsideWechatData(folder.localPath, wechatDataRoots);

  int? excludedWechatBytesFor(String folderId) => wechatExcludedBytes[folderId];

  BackupFoldersState copyWith({
    bool? loading,
    String? error,
    bool clearError = false,
    List<BackupFolderDto>? folders,
    Map<String, BackupFolderStateDto>? folderStates,
    List<BackupActivityDto>? activities,
    BackupSummaryDto? summary,
    List<String>? wechatDataRoots,
    Map<String, int>? wechatExcludedBytes,
  }) {
    return BackupFoldersState(
      loading: loading ?? this.loading,
      error: clearError ? null : (error ?? this.error),
      folders: folders ?? this.folders,
      folderStates: folderStates ?? this.folderStates,
      activities: activities ?? this.activities,
      summary: summary ?? this.summary,
      wechatDataRoots: wechatDataRoots ?? this.wechatDataRoots,
      wechatExcludedBytes: wechatExcludedBytes ?? this.wechatExcludedBytes,
    );
  }
}

/// 备份文件夹页 / 托盘共用的数据源。
///
/// 直接订阅桥接事件流（而不是 StreamProvider），保证同一帧内的多个活动或
/// 状态事件不会被合并丢失。
class BackupFoldersController extends Notifier<BackupFoldersState> {
  bool _loading = false;

  @override
  BackupFoldersState build() {
    final bridge = ref.watch(nativeBridgeProvider);
    final events = bridge.backupEventsStream().listen(_onEvent);
    ref.onDispose(() {
      events.cancel();
    });
    ref.listen(authControllerProvider, (previous, next) {
      if (next.stage == AuthStage.ready && previous?.stage != AuthStage.ready) {
        Future<void>.microtask(load);
      }
    });
    if (ref.read(authControllerProvider).stage == AuthStage.ready) {
      Future<void>.microtask(load);
    }
    return const BackupFoldersState(loading: true);
  }

  Future<void> load() async {
    if (_loading) {
      return;
    }
    _loading = true;
    state = state.copyWith(loading: true, clearError: true);
    final bridge = ref.read(nativeBridgeProvider);
    try {
      final results = await Future.wait<Object>([
        bridge.backupListFolders(),
        bridge.backupFolderStates(),
        bridge.backupRecentActivity(20),
        bridge.backupSummary(),
      ]);
      if (!ref.mounted) {
        return;
      }
      final folders = results[0] as List<BackupFolderDto>;
      final states = results[1] as List<BackupFolderStateDto>;
      final activities = results[2] as List<BackupActivityDto>;
      final summary = results[3] as BackupSummaryDto;
      var accounts = const <WechatAccountDto>[];
      try {
        accounts = await bridge.wechatListAccounts();
      } catch (_) {
        // 微信数据发现失败不影响普通备份文件夹页面。
      }
      if (!ref.mounted) {
        return;
      }
      final roots = <String>{for (final account in accounts) account.dataRoot}
          .toList(growable: false);
      final excluded = await _loadWechatExcludedBytes(bridge, folders, roots);
      if (!ref.mounted) {
        return;
      }
      state = BackupFoldersState(
        folders: folders,
        folderStates: <String, BackupFolderStateDto>{
          for (final item in states) item.folderId: item,
        },
        // 微信来源活动属于“微信备份”页面；这里只显示普通文件夹活动。
        activities: activities
            .where((item) => item.wechat == null)
            .toList(growable: false),
        summary: summary,
        wechatDataRoots: roots,
        wechatExcludedBytes: excluded,
      );
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(loading: false, error: bridgeErrorText(error));
    } finally {
      _loading = false;
    }
  }

  Future<void> pauseAll() => _run((bridge) async {
    await bridge.backupPauseAll();
  });

  Future<void> resumeAll() => _run((bridge) async {
    await bridge.backupResumeAll();
  });

  Future<void> pauseFolder(String folderId) => _run((bridge) async {
    await bridge.backupPauseFolder(folderId);
  });

  Future<void> resumeFolder(String folderId) => _run((bridge) async {
    await bridge.backupResumeFolder(folderId);
  });

  Future<void> scanAll() => _run((bridge) async {
    await bridge.backupScanAllNow();
  });

  Future<void> scanFolder(String folderId) => _run((bridge) async {
    await bridge.backupScanFolderNow(folderId);
  });

  Future<void> removeFolder(String folderId) => _run((bridge) async {
    await bridge.backupRemoveFolder(folderId);
  });

  /// 只为“内部包含微信数据根”的普通文件夹算预检；无关文件夹不做全量扫描。
  Future<Map<String, int>> _loadWechatExcludedBytes(
    NativeBridge bridge,
    List<BackupFolderDto> folders,
    List<String> dataRoots,
  ) async {
    if (dataRoots.isEmpty) {
      return const <String, int>{};
    }
    final candidates = folders
        .where((folder) => containsWechatDataRoot(folder.localPath, dataRoots))
        .toList(growable: false);
    if (candidates.isEmpty) {
      return const <String, int>{};
    }
    final excluded = <String, int>{};
    for (final folder in candidates) {
      try {
        final preflight = await bridge.backupPreflight(
          BackupFolderDraftDto(
            localPath: folder.localPath,
            excludeGlobs: folder.excludeGlobs,
            excludeSubfolders: folder.excludeSubfolders,
          ),
        );
        if (preflight.excludedWechatBytes > 0) {
          excluded[folder.id] = preflight.excludedWechatBytes;
        }
      } catch (_) {
        // 预检失败时只是不显示提示，不影响文件夹状态。
      }
    }
    return excluded;
  }

  Future<void> _run(Future<void> Function(NativeBridge bridge) action) async {
    try {
      await action(ref.read(nativeBridgeProvider));
      await load();
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(loading: false, error: bridgeErrorText(error));
    }
  }

  void _onEvent(BackupEventDto event) {
    if (!ref.mounted) {
      return;
    }
    final summary = event.summary;
    if (summary != null) {
      state = state.copyWith(summary: summary);
      return;
    }
    final folder = event.folder;
    if (folder != null) {
      if (folder.wechat != null) {
        return;
      }
      final next = <String, BackupFolderStateDto>{...state.folderStates};
      next[folder.folderId] = folder;
      state = state.copyWith(folderStates: next);
      return;
    }
    final activity = event.activity;
    if (activity != null) {
      // 微信来源活动属于“微信备份”页面。
      if (activity.wechat != null) {
        return;
      }
      if (state.activities.any((item) => item.id == activity.id)) {
        return;
      }
      state = state.copyWith(
        activities: <BackupActivityDto>[
          activity,
          ...state.activities,
        ].take(20).toList(growable: false),
      );
    }
  }
}

final backupFoldersProvider =
    NotifierProvider<BackupFoldersController, BackupFoldersState>(
      BackupFoldersController.new,
    );
