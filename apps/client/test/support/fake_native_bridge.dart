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

  final List<String> calls = <String>[];
  final List<String> pickedFiles = <String>[];

  final StreamController<TransferProgressDto> _progress =
      StreamController<TransferProgressDto>.broadcast();
  final StreamController<FolderProgressDto> _folderProgress =
      StreamController<FolderProgressDto>.broadcast();
  final StreamController<BackupEventDto> _backupEvents =
      StreamController<BackupEventDto>.broadcast();

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
  }) async {
    calls.add('initialize:$serverUrl');
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
    calls.add('startDownloadFolder:$folderId:$destinationRoot');
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
  Stream<BackupEventDto> backupEventsStream() => _backupEvents.stream;

  void emitProgress(TransferProgressDto event) => _progress.add(event);

  void emitFolderProgress(FolderProgressDto event) =>
      _folderProgress.add(event);

  Future<void> dispose() async {
    await _progress.close();
    await _folderProgress.close();
    await _backupEvents.close();
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
