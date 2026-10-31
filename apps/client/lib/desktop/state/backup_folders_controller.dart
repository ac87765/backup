import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../common/state/auth_controller.dart';
import '../../core/errors/bridge_error_text.dart';
import '../../core/providers/app_providers.dart';
import '../../core/rust/models.dart';
import '../../core/rust/native_bridge.dart';

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
  });

  final bool loading;
  final String? error;
  final List<BackupFolderDto> folders;
  final Map<String, BackupFolderStateDto> folderStates;
  final List<BackupActivityDto> activities;
  final BackupSummaryDto? summary;

  bool get isEmpty => !loading && folders.isEmpty && error == null;

  bool get globalPaused => summary?.kind == BackupSummaryStatusKindDto.paused;

  BackupFolderStateDto? stateFor(String folderId) => folderStates[folderId];

  BackupFoldersState copyWith({
    bool? loading,
    String? error,
    bool clearError = false,
    List<BackupFolderDto>? folders,
    Map<String, BackupFolderStateDto>? folderStates,
    List<BackupActivityDto>? activities,
    BackupSummaryDto? summary,
  }) {
    return BackupFoldersState(
      loading: loading ?? this.loading,
      error: clearError ? null : (error ?? this.error),
      folders: folders ?? this.folders,
      folderStates: folderStates ?? this.folderStates,
      activities: activities ?? this.activities,
      summary: summary ?? this.summary,
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
      state = BackupFoldersState(
        folders: folders,
        folderStates: <String, BackupFolderStateDto>{
          for (final item in states) item.folderId: item,
        },
        activities: activities,
        summary: summary,
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
      final next = <String, BackupFolderStateDto>{...state.folderStates};
      next[folder.folderId] = folder;
      state = state.copyWith(folderStates: next);
      return;
    }
    final activity = event.activity;
    if (activity != null) {
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
