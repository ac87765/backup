import 'dart:async';

import 'package:encrypted_backup_client/core/rust/models.dart';
import 'package:encrypted_backup_client/core/rust/native_bridge.dart';

AccountDto testAccount({
  String email = 'tlj978626571@gmail.com',
  String rootNodeId = 'root',
  AccountStatusDto status = AccountStatusDto.active,
  bool unlocked = true,
}) {
  return AccountDto(
    userId: 'user-1',
    email: email,
    emailVerified: true,
    rootNodeId: rootNodeId,
    status: status,
    statusReason: null,
    isAdmin: false,
    writesAllowed: status == AccountStatusDto.active && unlocked,
    unlocked: unlocked,
  );
}

NodeDto testNode({
  required String id,
  required String name,
  String? parentId = 'root',
  NodeKindDto kind = NodeKindDto.file,
  int? size = 1024,
  int? mtime,
  String? mime,
  int rowVersion = 1,
  String? currentVersionId,
  String? deletedAt,
  String? updatedAt,
}) {
  return NodeDto(
    id: id,
    parentId: parentId,
    kind: kind,
    name: name,
    size: kind == NodeKindDto.folder ? null : size,
    mtime: kind == NodeKindDto.folder
        ? null
        : (mtime ?? DateTime.now().millisecondsSinceEpoch ~/ 1000),
    mime: mime,
    rowVersion: rowVersion,
    currentVersionId: currentVersionId,
    createdAt: '2026-10-01T00:00:00Z',
    updatedAt: updatedAt ?? '2026-10-01T00:00:00Z',
    deletedAt: deletedAt,
    trashBatchId: null,
  );
}

VersionDto testVersion({
  required String id,
  int size = 1024,
  bool isCurrent = false,
  String createdAt = '2026-10-01T00:00:00Z',
  String contentId = 'content-1',
}) {
  return VersionDto(
    id: id,
    contentId: contentId,
    size: size,
    mtime: 1759276800,
    mime: 'application/octet-stream',
    isCurrent: isCurrent,
    createdAt: createdAt,
  );
}

typedef FakeWechatPreflightBuilder = WechatBackupPreflightDto Function(
  WechatAccountRefDto account,
  List<WechatCategoryDto> categories,
);

/// 组件/页面测试用的桥接假实现：不加载 Rust 库、不访问网络或系统钥匙串。
class FakeNativeBridge extends NativeBridge {
  FakeNativeBridge({
    AccountDto? account,
    this.storedSession = false,
    this.keepUnlocked = false,
    List<NodeDto>? rootItems,
  }) : account = account,
       sessionAccount = account {
    folders['root'] = rootItems ?? <NodeDto>[];
  }

  AccountDto? account;
  AccountDto? sessionAccount;
  bool storedSession;
  bool keepUnlocked;

  /// 登录/解锁返回的账号；null 时使用 [account]。
  AccountDto? loginAccount;
  AccountDto? unlockAccount;

  /// 非空时登录/解锁抛出对应桥接错误。
  BridgeError? loginError;
  BridgeError? unlockError;
  BridgeError? restoreError;

  /// 非空时登录挂起，用于截取“正在验证…”状态。
  Completer<AccountDto>? loginCompleter;

  final Map<String, List<NodeDto>> folders = <String, List<NodeDto>>{};
  final List<TrashItemDto> trash = <TrashItemDto>[];
  final Map<String, List<VersionDto>> versions = <String, List<VersionDto>>{};
  List<PendingUploadDto> pending = <PendingUploadDto>[];

  UsageDto usage = const UsageDto(
    usedBytes: 12400000000,
    reservedBytes: 0,
    quotaBytes: 50000000000,
  );
  BackupSummaryDto summary = const BackupSummaryDto(
    kind: BackupSummaryStatusKindDto.allBackedUp,
    done: 0,
    total: 0,
    unreadableCount: 0,
    folderCount: 3,
    lastCheckedAt: null,
  );

  List<BackupFolderDto> backupFolders = <BackupFolderDto>[];
  List<BackupFolderStateDto> backupStates = <BackupFolderStateDto>[];
  List<BackupActivityDto> backupActivities = <BackupActivityDto>[];
  BackupPreflightDto preflight = const BackupPreflightDto(
    fileCount: 1284,
    totalBytes: 3200000000,
    uploadBytes: 3200000000,
    excludedCount: 0,
    unreadableCount: 0,
    remainingQuotaBytes: 37600000000,
    excludedWechatFileCount: 0,
    excludedWechatBytes: 0,
  );
  BackupSettingsDto backupSettingsValue = const BackupSettingsDto(
    includeHidden: false,
    uploadLimitBytesPerSecond: null,
    paused: false,
    wechatSnapshotRetention: 3,
  );
  final Map<String, BackupPreflightDto> preflightByPath =
      <String, BackupPreflightDto>{};
  BackupFolderDraftDto? lastBackupDraft;
  BackupFolderUpdateDto? lastBackupUpdate;
  int _nextBackupFolder = 1;

  /// 微信备份假数据（第 4 组桥接层；界面在 4b 使用）。
  List<WechatAccountDto> wechatAccounts = <WechatAccountDto>[];
  List<WechatAccountDto>? wechatAccountsAfterManualAdd;
  List<WechatAccountBackupStatusDto> wechatStatuses =
      <WechatAccountBackupStatusDto>[];
  FakeWechatPreflightBuilder? wechatPreflightBuilder;
  BridgeError? wechatRestoreError;
  List<WechatDatabaseSnapshotDto> wechatSnapshots =
      <WechatDatabaseSnapshotDto>[];
  List<WechatRestoreCategoryDto> wechatRestoreCategoryList =
      <WechatRestoreCategoryDto>[];
  List<String> wechatManualDirs = <String>[];
  WechatAccountRefDto? lastWechatAccount;
  List<WechatCategoryDto>? lastWechatCategories;
  WechatRestoreRequestDto? lastWechatRestoreRequest;
  WechatBackupPreflightDto wechatPreflightValue =
      const WechatBackupPreflightDto(
        fileCount: 0,
        totalBytes: 0,
        uploadBytes: 0,
        uploadBytesWithoutImagesAndVideos: 0,
        databaseBytes: 0,
        excludedCount: 0,
        unreadableCount: 0,
        remainingQuotaBytes: null,
        overQuota: false,
      );
  WechatSnapshotReportDto wechatSnapshotReport = const WechatSnapshotReportDto(
    outcome: WechatSnapshotOutcomeDto.waitingForWechatExit,
    snapshotAt: null,
    totalFiles: 0,
    uploadedFiles: 0,
    uploadedBytes: 0,
  );

  final List<String> calls = <String>[];
  final List<String> pickedFiles = <String>[];

  /// 更新假数据（第 2 组桥接层；界面在 2b 使用）。
  UpdateCheckDto updateCheckResult = const UpdateCheckDto(
    state: UpdateCheckStateDto.upToDate,
    info: null,
    checkedAt: 0,
    reason: null,
  );
  String? dismissedUpdateVersion;
  BridgeError? updateDownloadError;
  BridgeError? updateInstallError;
  UpdateInstallOutcomeDto updateInstallResult = const UpdateInstallOutcomeDto(
    kind: UpdateInstallKindDto.started,
  );
  bool updateDownloadStarted = false;
  bool updateDmgOpened = false;

  final StreamController<TransferProgressDto> _progress =
      StreamController<TransferProgressDto>.broadcast();
  final StreamController<FolderProgressDto> _folderProgress =
      StreamController<FolderProgressDto>.broadcast();
  final StreamController<BackupEventDto> _backupEvents =
      StreamController<BackupEventDto>.broadcast();
  final StreamController<UpdateProgressDto> _updateProgress =
      StreamController<UpdateProgressDto>.broadcast();

  int _nextTransfer = 1;
  int _nextNode = 1;

  String nextTransferId() => 'transfer-${_nextTransfer++}';

  @override
  Future<void> ensureInitialized() async {}

  @override
  Future<void> initialize({
    required String appDataDir,
    required String serverUrl,
    required String deviceName,
    required String appVersion,
  }) async {
    calls.add('initialize:$serverUrl:$appVersion');
  }

  @override
  Future<AccountDto> login(String email, String password) async {
    calls.add('login:$email');
    final error = loginError;
    if (error != null) {
      throw error;
    }
    final completer = loginCompleter;
    if (completer != null) {
      return completer.future;
    }
    final result =
        loginAccount ?? account ?? testAccount(email: email, unlocked: true);
    account = result;
    sessionAccount = result;
    storedSession = true;
    return result;
  }

  @override
  Future<AccountDto> unlock(String password) async {
    calls.add('unlock');
    final error = unlockError;
    if (error != null) {
      throw error;
    }
    final result = unlockAccount ?? account ?? testAccount(unlocked: true);
    account = result;
    return result;
  }

  @override
  Future<AccountDto?> unlockWithStoredMasterKey() async {
    calls.add('unlockWithStoredMasterKey');
    if (!keepUnlocked) {
      return null;
    }
    return account;
  }

  @override
  Future<AccountDto?> restoreSession() async {
    calls.add('restoreSession');
    final error = restoreError;
    if (error != null) {
      throw error;
    }
    if (!storedSession) {
      return null;
    }
    return sessionAccount;
  }

  @override
  Future<AccountDto> currentAccount() async {
    calls.add('currentAccount');
    return account ?? testAccount();
  }

  @override
  Future<bool> hasStoredSession() async {
    calls.add('hasStoredSession');
    return storedSession;
  }

  @override
  Future<bool> isUnlocked() async => account?.unlocked ?? false;

  @override
  Future<bool> isKeepUnlocked() async {
    calls.add('isKeepUnlocked');
    return keepUnlocked;
  }

  @override
  Future<void> setKeepUnlocked(bool enabled) async {
    calls.add('setKeepUnlocked:$enabled');
    keepUnlocked = enabled;
  }

  @override
  Future<void> lock() async {
    calls.add('lock');
    account = account == null ? null : _locked(account!);
  }

  @override
  Future<void> logout() async {
    calls.add('logout');
    storedSession = false;
    keepUnlocked = false;
    account = null;
    sessionAccount = null;
  }

  @override
  Future<void> forgetCredentials() async {
    calls.add('forgetCredentials');
    storedSession = false;
    keepUnlocked = false;
    account = null;
    sessionAccount = null;
  }

  @override
  Future<List<NodeDto>> listFolder(String folderId, NodeSortDto sort) async {
    calls.add('listFolder:$folderId');
    final items = [...?folders[folderId]];
    items.sort((left, right) {
      if (left.kind != right.kind) {
        return left.kind == NodeKindDto.folder ? -1 : 1;
      }
      final factor = sort.descending ? -1 : 1;
      final primary = switch (sort.field) {
        NodeSortFieldDto.name => left.name.toLowerCase().compareTo(
          right.name.toLowerCase(),
        ),
        NodeSortFieldDto.modifiedTime => (left.mtime ?? 0).compareTo(
          right.mtime ?? 0,
        ),
        NodeSortFieldDto.size => (left.size ?? 0).compareTo(right.size ?? 0),
      };
      if (primary != 0) {
        return primary * factor;
      }
      return left.name.compareTo(right.name);
    });
    return items;
  }

  @override
  Future<NodeDto> createFolder(String parentId, String name) async {
    calls.add('createFolder:$parentId:$name');
    final node = testNode(
      id: 'new-node-${_nextNode++}',
      parentId: parentId,
      name: name,
      kind: NodeKindDto.folder,
    );
    (folders[parentId] ??= <NodeDto>[]).add(node);
    return node;
  }

  @override
  Future<NodeDto> renameNode({
    required String nodeId,
    required String parentId,
    required String newName,
    required int rowVersion,
  }) async {
    calls.add('renameNode:$nodeId:$newName');
    final node = _findNode(nodeId);
    if (node == null) {
      throw const BridgeError(code: 'not_found', message: '找不到文件');
    }
    final updated = _copyNode(node, name: newName, rowVersion: rowVersion + 1);
    _replaceNode(updated);
    return updated;
  }

  @override
  Future<NodeDto> moveNode({
    required String nodeId,
    required String currentParentId,
    required String newParentId,
    required String name,
    required int rowVersion,
  }) async {
    calls.add('moveNode:$nodeId:$newParentId');
    final node = _findNode(nodeId);
    if (node == null) {
      throw const BridgeError(code: 'not_found', message: '找不到文件');
    }
    folders[currentParentId]?.removeWhere((item) => item.id == nodeId);
    final updated = _copyNode(
      node,
      parentId: newParentId,
      name: name,
      rowVersion: rowVersion + 1,
    );
    (folders[newParentId] ??= <NodeDto>[]).add(updated);
    return updated;
  }

  @override
  Future<String> deleteToTrash(String nodeId) async {
    calls.add('deleteToTrash:$nodeId');
    final node = _findNode(nodeId);
    if (node == null) {
      throw const BridgeError(code: 'not_found', message: '找不到文件');
    }
    folders[node.parentId]?.removeWhere((item) => item.id == nodeId);
    trash.add(
      TrashItemDto(
        node: node,
        deletedAt: '2026-10-30T09:00:00Z',
        trashBatchId: 'batch-1',
      ),
    );
    return 'batch-1';
  }

  @override
  Future<List<TrashItemDto>> listTrash() async {
    calls.add('listTrash');
    return List<TrashItemDto>.of(trash);
  }

  @override
  Future<NodeDto> restoreFromTrash(
    String nodeId,
    RestoreTargetDto? target,
  ) async {
    calls.add('restoreFromTrash:$nodeId');
    final index = trash.indexWhere((item) => item.node.id == nodeId);
    if (index < 0) {
      throw const BridgeError(code: 'not_found', message: '找不到回收站项目');
    }
    final item = trash.removeAt(index);
    final parent = target?.parentId ?? item.node.parentId ?? 'root';
    final restored = _copyNode(
      item.node,
      parentId: parent,
      clearDeletedAt: true,
    );
    (folders[parent] ??= <NodeDto>[]).add(restored);
    return restored;
  }

  @override
  Future<PermanentDeleteOutcomeDto> permanentlyDelete(String nodeId) async {
    calls.add('permanentlyDelete:$nodeId');
    trash.removeWhere((item) => item.node.id == nodeId);
    return const PermanentDeleteOutcomeDto(
      deletedNodes: 1,
      deletedVersions: 1,
      deletedContents: 1,
      freedBytes: 2048,
      queuedObjects: 1,
    );
  }

  @override
  Future<List<VersionDto>> listVersions(String nodeId) async {
    calls.add('listVersions:$nodeId');
    return List<VersionDto>.of(versions[nodeId] ?? <VersionDto>[]);
  }

  @override
  Future<VersionDto> restoreVersion(
    String nodeId,
    String sourceVersionId,
  ) async {
    calls.add('restoreVersion:$nodeId:$sourceVersionId');
    final list = versions[nodeId] ?? <VersionDto>[];
    for (var index = 0; index < list.length; index += 1) {
      list[index] = testVersion(
        id: list[index].id,
        size: list[index].size,
        isCurrent: list[index].id == sourceVersionId,
        createdAt: list[index].createdAt,
        contentId: list[index].contentId,
      );
    }
    return list.firstWhere((version) => version.id == sourceVersionId);
  }

  @override
  Future<UsageDto> getUsage() async {
    calls.add('getUsage');
    return usage;
  }

  @override
  Future<String> startUploadFile(String parentId, String localPath) async {
    calls.add('startUploadFile:$localPath');
    pickedFiles.add(localPath);
    return nextTransferId();
  }

  @override
  Future<String> startUploadNewVersion(String nodeId, String localPath) async {
    calls.add('startUploadNewVersion:$nodeId:$localPath');
    pickedFiles.add(localPath);
    return nextTransferId();
  }

  @override
  Future<String> startUploadFolder(String parentId, String localDir) async {
    calls.add('startUploadFolder:$parentId:$localDir');
    return nextTransferId();
  }

  @override
  Future<String> startDownloadFile({
    required String nodeId,
    required String versionId,
    required String fileName,
    required String destinationDir,
    required ConflictPolicyDto policy,
  }) async {
    calls.add('startDownloadFile:$nodeId:$versionId:$destinationDir');
    return nextTransferId();
  }

  @override
  Future<String> startDownloadFolder({
    required String folderId,
    required String folderName,
    required String destinationRoot,
    required ConflictPolicyDto policy,
  }) async {
    calls.add('startDownloadFolder:$folderId:$destinationRoot:${policy.name}');
    return nextTransferId();
  }

  @override
  Future<List<PendingUploadDto>> listPendingUploads() async {
    calls.add('listPendingUploads');
    return List<PendingUploadDto>.of(pending);
  }

  @override
  Future<String> resumePendingUpload(String sessionId) async {
    calls.add('resumePendingUpload:$sessionId');
    pending.removeWhere((record) => record.sessionId == sessionId);
    return nextTransferId();
  }

  @override
  Future<void> abortPendingUpload(String sessionId) async {
    calls.add('abortPendingUpload:$sessionId');
    pending.removeWhere((record) => record.sessionId == sessionId);
  }

  @override
  Future<void> pauseTransfer(String transferId) async {
    calls.add('pauseTransfer:$transferId');
  }

  @override
  Future<void> resumeTransfer(String transferId) async {
    calls.add('resumeTransfer:$transferId');
  }

  @override
  Future<void> cancelTransfer(String transferId) async {
    calls.add('cancelTransfer:$transferId');
  }

  @override
  Future<void> retryTransfer(String transferId) async {
    calls.add('retryTransfer:$transferId');
  }

  @override
  Stream<TransferProgressDto> transferProgressStream() => _progress.stream;

  @override
  Stream<FolderProgressDto> folderProgressStream() => _folderProgress.stream;

  @override
  Future<BackupSummaryDto> backupSummary() async => summary;

  @override
  Future<void> backupStart() async {
    calls.add('backupStart');
  }

  @override
  Future<void> backupStop() async {
    calls.add('backupStop');
  }

  @override
  Future<List<BackupFolderDto>> backupListFolders() async {
    calls.add('backupListFolders');
    return List<BackupFolderDto>.of(backupFolders);
  }

  @override
  Future<BackupFolderDto> backupFolder(String folderId) async {
    calls.add('backupFolder:$folderId');
    return backupFolders.firstWhere((folder) => folder.id == folderId);
  }

  @override
  Future<BackupFolderDto> backupAddFolder(BackupFolderDraftDto draft) async {
    calls.add('backupAddFolder:${draft.localPath}');
    lastBackupDraft = draft;
    final name = _baseName(draft.localPath);
    final folder = BackupFolderDto(
      id: 'backup-${_nextBackupFolder++}',
      localPath: draft.localPath,
      localName: name,
      cloudTargetId: draft.cloudTargetId,
      cloudFolderId: 'cloud-$name',
      cloudPath: draft.cloudTargetId == null
          ? '全部文件 / 电脑备份 / DESKTOP-TLJ / $name'
          : '全部文件 / 目标 / $name',
      excludeGlobs: List<String>.of(draft.excludeGlobs),
      excludeSubfolders: List<String>.of(draft.excludeSubfolders),
      enabled: true,
      lastBackupAt: null,
    );
    backupFolders = <BackupFolderDto>[...backupFolders, folder];
    backupStates = <BackupFolderStateDto>[
      ...backupStates,
      BackupFolderStateDto(
        folderId: folder.id,
        status: const BackupFolderStatusDto(
          kind: BackupFolderStatusKindDto.idleOk,
          done: 0,
          total: 0,
          unreadableCount: 0,
        ),
        lastBackupAt: null,
        wechat: null,
      ),
    ];
    _refreshSummary();
    return folder;
  }

  @override
  Future<BackupFolderDto> backupUpdateFolder(
    String folderId,
    BackupFolderUpdateDto update,
  ) async {
    calls.add('backupUpdateFolder:$folderId');
    lastBackupUpdate = update;
    final index = backupFolders.indexWhere((folder) => folder.id == folderId);
    if (index < 0) {
      throw const BridgeError(code: 'not_found', message: '找不到备份文件夹');
    }
    final current = backupFolders[index];
    final updated = BackupFolderDto(
      id: current.id,
      localPath: update.localPath ?? current.localPath,
      localName: _baseName(update.localPath ?? current.localPath),
      cloudTargetId: update.resetCloudTarget
          ? null
          : (update.cloudTargetId ?? current.cloudTargetId),
      cloudFolderId: current.cloudFolderId,
      cloudPath: update.resetCloudTarget
          ? '全部文件 / 电脑备份 / DESKTOP-TLJ / ${current.localName}'
          : current.cloudPath,
      excludeGlobs: update.excludeGlobs ?? current.excludeGlobs,
      excludeSubfolders: update.excludeSubfolders ?? current.excludeSubfolders,
      enabled: update.enabled ?? current.enabled,
      lastBackupAt: current.lastBackupAt,
    );
    backupFolders = <BackupFolderDto>[...backupFolders]..[index] = updated;
    return updated;
  }

  @override
  Future<void> backupRemoveFolder(String folderId) async {
    calls.add('backupRemoveFolder:$folderId');
    backupFolders = backupFolders
        .where((folder) => folder.id != folderId)
        .toList(growable: false);
    backupStates = backupStates
        .where((state) => state.folderId != folderId)
        .toList(growable: false);
    _refreshSummary();
  }

  @override
  Future<BackupPreflightDto> backupPreflight(BackupFolderDraftDto draft) async {
    calls.add('backupPreflight:${draft.localPath}');
    lastBackupDraft = draft;
    return preflightByPath[draft.localPath] ?? preflight;
  }

  @override
  Future<void> backupPauseAll() async {
    calls.add('backupPauseAll');
    backupSettingsValue = BackupSettingsDto(
      includeHidden: backupSettingsValue.includeHidden,
      uploadLimitBytesPerSecond: backupSettingsValue.uploadLimitBytesPerSecond,
      paused: true,
      wechatSnapshotRetention: backupSettingsValue.wechatSnapshotRetention,
    );
    summary = BackupSummaryDto(
      kind: BackupSummaryStatusKindDto.paused,
      done: summary.done,
      total: summary.total,
      unreadableCount: summary.unreadableCount,
      folderCount: summary.folderCount,
      lastCheckedAt: summary.lastCheckedAt,
    );
  }

  @override
  Future<void> backupResumeAll() async {
    calls.add('backupResumeAll');
    backupSettingsValue = BackupSettingsDto(
      includeHidden: backupSettingsValue.includeHidden,
      uploadLimitBytesPerSecond: backupSettingsValue.uploadLimitBytesPerSecond,
      paused: false,
      wechatSnapshotRetention: backupSettingsValue.wechatSnapshotRetention,
    );
    summary = BackupSummaryDto(
      kind: BackupSummaryStatusKindDto.allBackedUp,
      done: summary.done,
      total: summary.total,
      unreadableCount: summary.unreadableCount,
      folderCount: summary.folderCount,
      lastCheckedAt: summary.lastCheckedAt,
    );
  }

  @override
  Future<void> backupPauseFolder(String folderId) async {
    calls.add('backupPauseFolder:$folderId');
    _setFolderEnabled(folderId, false);
    _setFolderStatus(folderId, BackupFolderStatusKindDto.paused);
  }

  @override
  Future<void> backupResumeFolder(String folderId) async {
    calls.add('backupResumeFolder:$folderId');
    _setFolderEnabled(folderId, true);
    _setFolderStatus(folderId, BackupFolderStatusKindDto.idleOk);
  }

  @override
  Future<BackupSettingsDto> backupSettings() async {
    calls.add('backupSettings');
    return backupSettingsValue;
  }

  @override
  Future<BackupSettingsDto> backupCachedSettings() async {
    calls.add('backupCachedSettings');
    return backupSettingsValue;
  }

  @override
  Future<void> backupSetSettings(BackupSettingsDto settings) async {
    calls.add(
      'backupSetSettings:includeHidden=${settings.includeHidden}:'
      'limit=${settings.uploadLimitBytesPerSecond}',
    );
    backupSettingsValue = settings;
  }

  @override
  Future<List<BackupFolderStateDto>> backupFolderStates() async {
    calls.add('backupFolderStates');
    return List<BackupFolderStateDto>.of(backupStates);
  }

  @override
  Future<List<BackupActivityDto>> backupRecentActivity(int limit) async {
    calls.add('backupRecentActivity:$limit');
    return backupActivities.take(limit).toList(growable: false);
  }

  @override
  Future<BackupRunReportDto> backupScanFolderNow(String folderId) async {
    calls.add('backupScanFolderNow:$folderId');
    return BackupRunReportDto(
      folderId: folderId,
      status: const BackupFolderStatusDto(
        kind: BackupFolderStatusKindDto.idleOk,
        done: 0,
        total: 0,
        unreadableCount: 0,
      ),
      scannedFiles: 0,
      hashedFiles: 0,
      uploadedFiles: 0,
      updatedFiles: 0,
      unchangedFiles: 0,
      unreadableFiles: 0,
      retryLater: 0,
      failedFiles: 0,
      skippedExcluded: 0,
    );
  }

  @override
  Future<List<BackupRunReportDto>> backupScanAllNow() async {
    calls.add('backupScanAllNow');
    return <BackupRunReportDto>[
      for (final folder in backupFolders) await backupScanFolderNow(folder.id),
    ];
  }

  @override
  Stream<BackupEventDto> backupEventsStream() => _backupEvents.stream;

  void emitBackupEvent(BackupEventDto event) => _backupEvents.add(event);

  void emitBackupSummary(BackupSummaryDto value) {
    summary = value;
    emitBackupEvent(
      BackupEventDto(kind: BackupEventKindDto.summary, summary: value),
    );
  }

  void _refreshSummary() {
    summary = BackupSummaryDto(
      kind: summary.kind,
      done: summary.done,
      total: summary.total,
      unreadableCount: summary.unreadableCount,
      folderCount: backupFolders.length,
      lastCheckedAt: summary.lastCheckedAt,
    );
  }

  void _setFolderEnabled(String folderId, bool enabled) {
    final index = backupFolders.indexWhere((folder) => folder.id == folderId);
    if (index < 0) {
      return;
    }
    final current = backupFolders[index];
    backupFolders = <BackupFolderDto>[...backupFolders]
      ..[index] = BackupFolderDto(
        id: current.id,
        localPath: current.localPath,
        localName: current.localName,
        cloudTargetId: current.cloudTargetId,
        cloudFolderId: current.cloudFolderId,
        cloudPath: current.cloudPath,
        excludeGlobs: current.excludeGlobs,
        excludeSubfolders: current.excludeSubfolders,
        enabled: enabled,
        lastBackupAt: current.lastBackupAt,
      );
  }

  void _setFolderStatus(String folderId, BackupFolderStatusKindDto kind) {
    final index = backupStates.indexWhere(
      (state) => state.folderId == folderId,
    );
    if (index < 0) {
      return;
    }
    final current = backupStates[index];
    backupStates = <BackupFolderStateDto>[...backupStates]
      ..[index] = BackupFolderStateDto(
        folderId: folderId,
        status: BackupFolderStatusDto(
          kind: kind,
          done: current.status.done,
          total: current.status.total,
          unreadableCount: current.status.unreadableCount,
        ),
        pauseReason: kind == BackupFolderStatusKindDto.paused
            ? BackupPauseReasonDto.user
            : null,
        lastBackupAt: current.lastBackupAt,
        wechat: current.wechat,
      );
  }

  // ---- 微信备份 ----
  @override
  Future<List<WechatAccountDto>> wechatListAccounts() async {
    calls.add('wechatListAccounts');
    return List<WechatAccountDto>.of(wechatAccounts);
  }

  @override
  Future<List<WechatAccountDto>> wechatAddManualDirectory(String path) async {
    calls.add('wechatAddManualDirectory:$path');
    if (!wechatManualDirs.contains(path)) {
      wechatManualDirs = <String>[...wechatManualDirs, path];
    }
    final afterAdd = wechatAccountsAfterManualAdd;
    if (afterAdd != null) {
      wechatAccounts = List<WechatAccountDto>.of(afterAdd);
      wechatAccountsAfterManualAdd = null;
    }
    return wechatListAccounts();
  }

  @override
  Future<List<WechatAccountDto>> wechatRemoveManualDirectory(
    String path,
  ) async {
    calls.add('wechatRemoveManualDirectory:$path');
    wechatManualDirs = wechatManualDirs
        .where((directory) => directory != path)
        .toList(growable: false);
    return wechatListAccounts();
  }

  @override
  Future<List<String>> wechatManualDirectories() async {
    calls.add('wechatManualDirectories');
    return List<String>.of(wechatManualDirs);
  }

  @override
  Future<void> wechatSetNote(WechatAccountRefDto account, String note) async {
    calls.add('wechatSetNote:${account.accountName}');
    lastWechatAccount = account;
    wechatAccounts = <WechatAccountDto>[
      for (final item in wechatAccounts)
        if (item.accountName == account.accountName &&
            item.dataRoot == account.dataRoot)
          _copyWechatAccount(item, note: note)
        else
          item,
    ];
  }

  @override
  Future<void> wechatClearNote(WechatAccountRefDto account) async {
    calls.add('wechatClearNote:${account.accountName}');
    lastWechatAccount = account;
    wechatAccounts = <WechatAccountDto>[
      for (final item in wechatAccounts)
        if (item.accountName == account.accountName &&
            item.dataRoot == account.dataRoot)
          _copyWechatAccount(item, clearNote: true)
        else
          item,
    ];
  }

  @override
  Future<void> wechatSetCategories(
    WechatAccountRefDto account,
    List<WechatCategoryDto> categories,
  ) async {
    calls.add('wechatSetCategories:${account.accountName}');
    lastWechatAccount = account;
    lastWechatCategories = List<WechatCategoryDto>.of(categories);
    _applyWechatCategories(account, categories);
  }

  @override
  Future<WechatBackupPreflightDto> wechatPreflight(
    WechatAccountRefDto account,
    List<WechatCategoryDto> categories,
  ) async {
    calls.add('wechatPreflight:${account.accountName}');
    lastWechatAccount = account;
    lastWechatCategories = List<WechatCategoryDto>.of(categories);
    final builder = wechatPreflightBuilder;
    if (builder != null) {
      return builder(account, categories);
    }
    return wechatPreflightValue;
  }

  @override
  Future<List<WechatAccountBackupStatusDto>> wechatAccountStatuses() async {
    calls.add('wechatAccountStatuses');
    return List<WechatAccountBackupStatusDto>.of(wechatStatuses);
  }

  @override
  Future<WechatSnapshotReportDto> wechatSnapshotNow(
    WechatAccountRefDto account,
  ) async {
    calls.add('wechatSnapshotNow:${account.accountName}');
    lastWechatAccount = account;
    return wechatSnapshotReport;
  }

  @override
  Future<List<WechatDatabaseSnapshotDto>> wechatDatabaseSnapshots(
    WechatAccountRefDto account,
  ) async {
    calls.add('wechatDatabaseSnapshots:${account.accountName}');
    lastWechatAccount = account;
    return List<WechatDatabaseSnapshotDto>.of(wechatSnapshots);
  }

  @override
  Future<List<WechatRestoreCategoryDto>> wechatRestoreCategories(
    WechatAccountRefDto account,
  ) async {
    calls.add('wechatRestoreCategories:${account.accountName}');
    lastWechatAccount = account;
    return List<WechatRestoreCategoryDto>.of(wechatRestoreCategoryList);
  }

  @override
  Future<String> wechatStartRestore(WechatRestoreRequestDto request) async {
    calls.add('wechatStartRestore:${request.accountName}');
    final error = wechatRestoreError;
    if (error != null) {
      throw error;
    }
    lastWechatRestoreRequest = request;
    return nextTransferId();
  }

  WechatAccountDto _copyWechatAccount(
    WechatAccountDto account, {
    String? note,
    bool clearNote = false,
  }) {
    return WechatAccountDto(
      accountName: account.accountName,
      version: account.version,
      dataRoot: account.dataRoot,
      accountDir: account.accountDir,
      source: account.source,
      categories: account.categories,
      totalSizeBytes: account.totalSizeBytes,
      totalFileCount: account.totalFileCount,
      lastModifiedNs: account.lastModifiedNs,
      isCurrentLoginCandidate: account.isCurrentLoginCandidate,
      hasPhoneMigrationBackup: account.hasPhoneMigrationBackup,
      note: clearNote ? null : (note ?? account.note),
    );
  }

  WechatCategoryBackupStatusDto? _wechatCategoryStatus(
    WechatAccountBackupStatusDto status,
    WechatCategoryDto category,
  ) {
    for (final item in status.categories) {
      if (item.category == category) {
        return item;
      }
    }
    return null;
  }

  void _applyWechatCategories(
    WechatAccountRefDto account,
    List<WechatCategoryDto> categories,
  ) {
    final selected = categories.toSet();
    final index = wechatStatuses.indexWhere(
      (item) =>
          item.accountName == account.accountName &&
          item.dataRoot == account.dataRoot,
    );
    final existing = index < 0 ? null : wechatStatuses[index];
    final nextCategories = <WechatCategoryBackupStatusDto>[];
    for (final category in WechatCategoryDto.values) {
      final old = existing == null
          ? null
          : _wechatCategoryStatus(existing, category);
      final status =
          old?.status ??
          (selected.contains(category) &&
                  category == WechatCategoryDto.databases
              ? const BackupFolderStatusDto(
                  kind: BackupFolderStatusKindDto.waitingForWechatExit,
                  done: 0,
                  total: 0,
                  unreadableCount: 0,
                )
              : null);
      nextCategories.add(
        WechatCategoryBackupStatusDto(
          category: category,
          selected: selected.contains(category),
          status: status,
          pauseReason: old?.pauseReason,
          lastBackupAt: old?.lastBackupAt,
        ),
      );
    }
    final status = WechatAccountBackupStatusDto(
      dataRoot: account.dataRoot,
      accountName: account.accountName,
      version: account.version,
      categories: nextCategories,
      lastBackupAt: existing?.lastBackupAt,
    );
    if (index < 0) {
      wechatStatuses = <WechatAccountBackupStatusDto>[
        ...wechatStatuses,
        status,
      ];
    } else {
      wechatStatuses = <WechatAccountBackupStatusDto>[...wechatStatuses]
        ..[index] = status;
    }
  }

  String _baseName(String path) {
    final normalized = path.replaceAll('\\', '/');
    final trimmed = normalized.endsWith('/')
        ? normalized.substring(0, normalized.length - 1)
        : normalized;
    final index = trimmed.lastIndexOf('/');
    return index < 0 ? trimmed : trimmed.substring(index + 1);
  }

  void emitProgress(TransferProgressDto event) => _progress.add(event);

  void emitFolderProgress(FolderProgressDto event) =>
      _folderProgress.add(event);

  // ---- 客户端更新 ----
  @override
  Future<UpdateCheckDto> updateCheck(int now) async {
    calls.add('updateCheck:$now');
    return updateCheckResult;
  }

  @override
  Future<String?> updateBannerDismissedVersion() async {
    calls.add('updateBannerDismissedVersion');
    return dismissedUpdateVersion;
  }

  @override
  Future<void> updateDismissBanner(String version) async {
    calls.add('updateDismissBanner:$version');
    dismissedUpdateVersion = version;
  }

  @override
  Future<String> updateDownload() async {
    calls.add('updateDownload');
    final error = updateDownloadError;
    if (error != null) {
      throw error;
    }
    updateDownloadStarted = true;
    return 'update-1';
  }

  @override
  Future<void> updateCancelDownload() async {
    calls.add('updateCancelDownload');
    updateDownloadStarted = false;
  }

  @override
  Future<UpdateInstallOutcomeDto> updateInstall() async {
    calls.add('updateInstall');
    final error = updateInstallError;
    if (error != null) {
      throw error;
    }
    return updateInstallResult;
  }

  @override
  Future<void> updateOpenDmg() async {
    calls.add('updateOpenDmg');
    updateDmgOpened = true;
  }

  @override
  Stream<UpdateProgressDto> updateProgressStream() => _updateProgress.stream;

  void emitUpdateProgress(UpdateProgressDto event) =>
      _updateProgress.add(event);

  Future<void> dispose() async {
    await _progress.close();
    await _folderProgress.close();
    await _backupEvents.close();
    await _updateProgress.close();
  }

  NodeDto? _findNode(String id) {
    for (final list in folders.values) {
      for (final node in list) {
        if (node.id == id) {
          return node;
        }
      }
    }
    return null;
  }

  void _replaceNode(NodeDto updated) {
    for (final list in folders.values) {
      final index = list.indexWhere((node) => node.id == updated.id);
      if (index >= 0) {
        list[index] = updated;
        return;
      }
    }
  }

  NodeDto _copyNode(
    NodeDto node, {
    String? name,
    String? parentId,
    int? rowVersion,
    String? deletedAt,
    bool clearDeletedAt = false,
  }) {
    return NodeDto(
      id: node.id,
      parentId: parentId ?? node.parentId,
      kind: node.kind,
      name: name ?? node.name,
      size: node.size,
      mtime: node.mtime,
      mime: node.mime,
      rowVersion: rowVersion ?? node.rowVersion,
      currentVersionId: node.currentVersionId,
      createdAt: node.createdAt,
      updatedAt: node.updatedAt,
      deletedAt: clearDeletedAt ? null : (deletedAt ?? node.deletedAt),
      trashBatchId: node.trashBatchId,
    );
  }

  AccountDto _locked(AccountDto account) {
    return AccountDto(
      userId: account.userId,
      email: account.email,
      emailVerified: account.emailVerified,
      rootNodeId: account.rootNodeId,
      status: account.status,
      statusReason: account.statusReason,
      isAdmin: account.isAdmin,
      writesAllowed: false,
      unlocked: false,
    );
  }
}
