import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../common/state/auth_controller.dart';
import '../../core/errors/bridge_error_text.dart';
import '../../core/providers/app_providers.dart';
import '../../core/rust/models.dart';
import 'wechat_status.dart';

/// 首次开启前的汇总预检（多个账号时按账号相加，剩余配额共用）。
@immutable
class WechatSetupPreflight {
  const WechatSetupPreflight({
    required this.fileCount,
    required this.uploadBytes,
    required this.uploadBytesWithoutImagesAndVideos,
    required this.remainingQuotaBytes,
    required this.overQuota,
  });

  final int fileCount;
  final int uploadBytes;
  final int uploadBytesWithoutImagesAndVideos;
  final int? remainingQuotaBytes;
  final bool overQuota;

  static const empty = WechatSetupPreflight(
    fileCount: 0,
    uploadBytes: 0,
    uploadBytesWithoutImagesAndVideos: 0,
    remainingQuotaBytes: null,
    overQuota: false,
  );
}

@immutable
class WechatPageState {
  const WechatPageState({
    this.loading = false,
    this.error,
    this.busy = false,
    this.preflightLoading = false,
    this.preflightError,
    this.accounts = const <WechatAccountDto>[],
    this.statuses = const <WechatAccountBackupStatusDto>[],
    this.summary,
    this.setupCategories = const <WechatCategoryDto>{},
    this.setupPreflight,
  });

  final bool loading;
  final String? error;
  final bool busy;

  final bool preflightLoading;
  final String? preflightError;

  final List<WechatAccountDto> accounts;
  final List<WechatAccountBackupStatusDto> statuses;
  final BackupSummaryDto? summary;

  /// 首次开启前界面上尚未保存的勾选。
  final Set<WechatCategoryDto> setupCategories;
  final WechatSetupPreflight? setupPreflight;

  bool get hasAccounts => accounts.isNotEmpty;

  /// 至少一个账号保存过“已勾选类别”；否则显示首次开启卡片。
  bool get hasEnabledAccounts => statuses.any(wechatAccountHasSelection);

  bool get globalPaused => summary?.kind == BackupSummaryStatusKindDto.paused;

  bool get quotaExceeded => setupPreflight?.overQuota ?? false;

  List<WechatAccountDto> get enabledAccounts {
    return accounts
        .where((account) => wechatAccountHasSelection(statusFor(account)))
        .toList(growable: false);
  }

  WechatAccountBackupStatusDto? statusFor(WechatAccountDto account) {
    for (final status in statuses) {
      if (status.accountName == account.accountName &&
          status.dataRoot == account.dataRoot) {
        return status;
      }
    }
    return null;
  }

  WechatPageState copyWith({
    bool? loading,
    String? error,
    bool clearError = false,
    bool? busy,
    bool? preflightLoading,
    String? preflightError,
    bool clearPreflightError = false,
    List<WechatAccountDto>? accounts,
    List<WechatAccountBackupStatusDto>? statuses,
    BackupSummaryDto? summary,
    Set<WechatCategoryDto>? setupCategories,
    WechatSetupPreflight? setupPreflight,
    bool clearSetupPreflight = false,
  }) {
    return WechatPageState(
      loading: loading ?? this.loading,
      error: clearError ? null : (error ?? this.error),
      busy: busy ?? this.busy,
      preflightLoading: preflightLoading ?? this.preflightLoading,
      preflightError: clearPreflightError
          ? null
          : (preflightError ?? this.preflightError),
      accounts: accounts ?? this.accounts,
      statuses: statuses ?? this.statuses,
      summary: summary ?? this.summary,
      setupCategories: setupCategories ?? this.setupCategories,
      setupPreflight: clearSetupPreflight
          ? null
          : (setupPreflight ?? this.setupPreflight),
    );
  }
}

/// “微信备份”页的数据源：发现账号、保存的类别、各类别状态与全局暂停。
///
/// 直接订阅备份事件流（而不是 StreamProvider）：微信来源的文件夹状态事件带
/// 账号与类别，可以就地更新该类别状态，不必为每个文件事件重新扫描目录。
class WechatController extends Notifier<WechatPageState> {
  bool _loading = false;
  String _setupKey = '';
  int _preflightToken = 0;

  @override
  WechatPageState build() {
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
    return const WechatPageState(loading: true);
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
        bridge.wechatListAccounts(),
        bridge.wechatAccountStatuses(),
        bridge.backupSummary(),
      ]);
      if (!ref.mounted) {
        return;
      }
      final accounts = results[0] as List<WechatAccountDto>;
      final statuses = results[1] as List<WechatAccountBackupStatusDto>;
      final summary = results[2] as BackupSummaryDto;
      final key = accounts
          .map((account) => '${account.dataRoot}|${account.accountName}')
          .join('\n');
      final changed = key != _setupKey;
      if (changed) {
        _setupKey = key;
      }
      final setupCategories = changed
          ? wechatDefaultCategories(accounts)
          : state.setupCategories;
      state = WechatPageState(
        accounts: accounts,
        statuses: statuses,
        summary: summary,
        setupCategories: setupCategories,
        setupPreflight: changed ? null : state.setupPreflight,
      );
      if (!state.hasEnabledAccounts) {
        unawaited(_refreshPreflight());
      }
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(loading: false, error: bridgeErrorText(error));
    } finally {
      _loading = false;
    }
  }

  Future<void> addManualDirectory(String path) async {
    state = state.copyWith(busy: true, clearError: true);
    try {
      await ref.read(nativeBridgeProvider).wechatAddManualDirectory(path);
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(busy: false);
      await load();
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(busy: false, error: bridgeErrorText(error));
    }
  }

  Future<void> toggleSetupCategory(WechatCategoryDto category) async {
    final next = <WechatCategoryDto>{...state.setupCategories};
    if (!next.remove(category)) {
      next.add(category);
    }
    state = state.copyWith(setupCategories: next, clearPreflightError: true);
    await _refreshPreflight();
  }

  Future<void> startSetup() async {
    if (state.busy || state.setupCategories.isEmpty) {
      return;
    }
    state = state.copyWith(busy: true, clearError: true);
    try {
      final bridge = ref.read(nativeBridgeProvider);
      for (final account in state.accounts) {
        final categories = _selectedCategoriesFor(account);
        if (categories.isEmpty) {
          continue;
        }
        await bridge.wechatSetCategories(wechatAccountRef(account), categories);
      }
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(busy: false);
      await load();
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(busy: false, error: bridgeErrorText(error));
    }
  }

  Future<void> setAccountCategory(
    WechatAccountDto account,
    WechatCategoryDto category,
    bool selected,
  ) async {
    final status = state.statusFor(account);
    final current = <WechatCategoryDto>{
      for (final item
          in status?.categories ?? const <WechatCategoryBackupStatusDto>[])
        if (item.selected) item.category,
    };
    if (selected) {
      current.add(category);
    } else {
      current.remove(category);
    }
    state = state.copyWith(busy: true, clearError: true);
    try {
      await ref
          .read(nativeBridgeProvider)
          .wechatSetCategories(
            wechatAccountRef(account),
            current.toList(growable: false),
          );
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(busy: false);
      await load();
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(busy: false, error: bridgeErrorText(error));
    }
  }

  Future<void> setNote(WechatAccountDto account, String note) async {
    try {
      await ref
          .read(nativeBridgeProvider)
          .wechatSetNote(wechatAccountRef(account), note);
      if (ref.mounted) {
        await load();
      }
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(error: bridgeErrorText(error));
    }
  }

  Future<void> clearNote(WechatAccountDto account) async {
    try {
      await ref
          .read(nativeBridgeProvider)
          .wechatClearNote(wechatAccountRef(account));
      if (ref.mounted) {
        await load();
      }
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(error: bridgeErrorText(error));
    }
  }

  Future<void> pause() async {
    await _run(() => ref.read(nativeBridgeProvider).backupPauseAll());
  }

  Future<void> resume() async {
    await _run(() => ref.read(nativeBridgeProvider).backupResumeAll());
  }

  /// “立即备份”：文件类别走现有备份引擎的全量立即核对，数据库类别再逐账号
  /// 请求快照（微信运行时会返回“等微信退出后备份”）。
  Future<void> scanNow() async {
    if (state.busy || state.enabledAccounts.isEmpty) {
      return;
    }
    state = state.copyWith(busy: true, clearError: true);
    try {
      final bridge = ref.read(nativeBridgeProvider);
      await bridge.backupScanAllNow();
      for (final account in state.enabledAccounts) {
        final status = state.statusFor(account);
        final database = wechatCategoryStatus(
          status,
          WechatCategoryDto.databases,
        );
        if (database?.selected == true) {
          await bridge.wechatSnapshotNow(wechatAccountRef(account));
        }
      }
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(busy: false);
      await load();
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(busy: false, error: bridgeErrorText(error));
    }
  }

  Future<void> _run(Future<void> Function() action) async {
    try {
      await action();
      if (ref.mounted) {
        await load();
      }
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(error: bridgeErrorText(error));
    }
  }

  List<WechatCategoryDto> _selectedCategoriesFor(WechatAccountDto account) {
    final result = <WechatCategoryDto>[];
    for (final category in state.setupCategories) {
      final info = wechatCategoryInfo(account, category);
      // 手机迁移备份只在账号确实有数据时注册；数据库等类别即使暂时没有文件
      // 也允许注册，由引擎在运行/退出时处理。
      if (info != null &&
          (info.exists || category == WechatCategoryDto.databases)) {
        result.add(category);
      }
    }
    return result;
  }

  Future<void> _refreshPreflight() async {
    final token = ++_preflightToken;
    final accounts = state.accounts;
    if (accounts.isEmpty) {
      state = state.copyWith(
        preflightLoading: false,
        setupPreflight: WechatSetupPreflight.empty,
      );
      return;
    }
    state = state.copyWith(preflightLoading: true, clearPreflightError: true);
    final bridge = ref.read(nativeBridgeProvider);
    var fileCount = 0;
    var uploadBytes = 0;
    var withoutImages = 0;
    var overQuota = false;
    int? remaining;
    try {
      for (final account in accounts) {
        final categories = _selectedCategoriesFor(account);
        if (categories.isEmpty) {
          continue;
        }
        final preflight = await bridge.wechatPreflight(
          wechatAccountRef(account),
          categories,
        );
        fileCount += preflight.fileCount;
        uploadBytes += preflight.uploadBytes;
        withoutImages += preflight.uploadBytesWithoutImagesAndVideos;
        overQuota = overQuota || preflight.overQuota;
        remaining ??= preflight.remainingQuotaBytes;
      }
      if (!ref.mounted || token != _preflightToken) {
        return;
      }
      if (remaining != null && uploadBytes > remaining) {
        overQuota = true;
      }
      state = state.copyWith(
        preflightLoading: false,
        setupPreflight: WechatSetupPreflight(
          fileCount: fileCount,
          uploadBytes: uploadBytes,
          uploadBytesWithoutImagesAndVideos: withoutImages,
          remainingQuotaBytes: remaining,
          overQuota: overQuota,
        ),
      );
    } catch (error) {
      if (!ref.mounted || token != _preflightToken) {
        return;
      }
      state = state.copyWith(
        preflightLoading: false,
        preflightError: bridgeErrorText(error),
      );
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
    if (folder != null && folder.wechat != null) {
      _applyFolderEvent(folder);
      return;
    }
    final activity = event.activity;
    if (activity != null && activity.wechat != null) {
      _applyActivityEvent(activity);
    }
  }

  void _applyFolderEvent(BackupFolderStateDto folder) {
    final source = folder.wechat!;
    var changed = false;
    final statuses = <WechatAccountBackupStatusDto>[];
    for (final account in state.statuses) {
      if (account.accountName != source.accountName) {
        statuses.add(account);
        continue;
      }
      final categories = <WechatCategoryBackupStatusDto>[];
      for (final item in account.categories) {
        if (item.category != source.category) {
          categories.add(item);
          continue;
        }
        changed = true;
        categories.add(
          WechatCategoryBackupStatusDto(
            category: item.category,
            selected: true,
            status: folder.status,
            pauseReason: folder.pauseReason,
            lastBackupAt: folder.lastBackupAt ?? item.lastBackupAt,
          ),
        );
      }
      statuses.add(
        WechatAccountBackupStatusDto(
          dataRoot: account.dataRoot,
          accountName: account.accountName,
          version: account.version,
          categories: categories,
          lastBackupAt: _latestBackup(categories),
        ),
      );
    }
    if (changed) {
      state = state.copyWith(statuses: statuses);
    }
  }

  void _applyActivityEvent(BackupActivityDto activity) {
    final source = activity.wechat!;
    var changed = false;
    final statuses = <WechatAccountBackupStatusDto>[];
    for (final account in state.statuses) {
      if (account.accountName != source.accountName) {
        statuses.add(account);
        continue;
      }
      final categories = <WechatCategoryBackupStatusDto>[];
      for (final item in account.categories) {
        if (item.category != source.category) {
          categories.add(item);
          continue;
        }
        changed = true;
        categories.add(
          WechatCategoryBackupStatusDto(
            category: item.category,
            selected: true,
            status: item.status,
            pauseReason: item.pauseReason,
            lastBackupAt: activity.occurredAt,
          ),
        );
      }
      statuses.add(
        WechatAccountBackupStatusDto(
          dataRoot: account.dataRoot,
          accountName: account.accountName,
          version: account.version,
          categories: categories,
          lastBackupAt: _latestBackup(categories),
        ),
      );
    }
    if (changed) {
      state = state.copyWith(statuses: statuses);
    }
  }

  int? _latestBackup(List<WechatCategoryBackupStatusDto> categories) {
    int? latest;
    for (final item in categories) {
      final value = item.lastBackupAt;
      if (value != null && (latest == null || value > latest)) {
        latest = value;
      }
    }
    return latest;
  }
}

final wechatControllerProvider =
    NotifierProvider<WechatController, WechatPageState>(WechatController.new);

/// 从微信页面发起的恢复也复用传输面板。
WechatRestoreRequestDto wechatRestoreRequest({
  required WechatAccountDto account,
  required List<WechatCategoryDto> categories,
  required bool restoreDatabase,
  required WechatSnapshotSelectionDto? snapshot,
  String? dataRoot,
}) {
  return WechatRestoreRequestDto(
    dataRoot: dataRoot ?? account.dataRoot,
    accountName: account.accountName,
    version: account.version,
    categories: categories,
    restoreDatabase: restoreDatabase,
    snapshot: snapshot,
  );
}

/// 把桥接错误映射成恢复对话框里的中文提示；微信运行中给出更明确的下一步。
String wechatRestoreErrorText(Object error) {
  if (error is BridgeError && error.code == 'wechat_running') {
    return '请先完全退出微信（包括托盘图标），再开始恢复。';
  }
  return bridgeErrorText(error);
}
