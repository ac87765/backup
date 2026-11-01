import '../../src/rust/frb_generated.dart' show RustLib;
import '../../src/rust/api/auth.dart' as auth;
import '../../src/rust/api/backup.dart' as backup;
import '../../src/rust/api/files.dart' as files;
import '../../src/rust/api/init.dart' as init;
import '../../src/rust/api/transfer.dart' as transfer;
import '../../src/rust/api/update.dart' as update;
import '../../src/rust/api/wechat.dart' as wechat;
import 'models.dart';

/// 桥接 API 的薄封装：界面与 provider 只依赖这个类，不直接散落调用生成函数。
///
/// 这里不做业务判断、不缓存数据；所有参数与返回值都是生成的 DTO。密钥始终
/// 留在 Rust 侧。
class NativeBridge {
  const NativeBridge();

  // ---- 初始化 ----
  /// 初始化 flutter_rust_bridge 运行时。幂等，可在启动流程中重复调用。
  Future<void> ensureInitialized() => RustLib.init();

  Future<void> initialize({
    required String appDataDir,
    required String serverUrl,
    required String deviceName,
    required String appVersion,
  }) {
    return init.initialize(
      appDataDir: appDataDir,
      serverUrl: serverUrl,
      deviceName: deviceName,
      appVersion: appVersion,
    );
  }

  // ---- 认证 ----
  Future<KdfParamsDto> prelogin(String email) => auth.prelogin(email: email);

  Future<AccountDto> login(String email, String password) =>
      auth.login(email: email, password: password);

  Future<AccountDto> unlock(String password) => auth.unlock(password: password);

  Future<AccountDto?> unlockWithStoredMasterKey() =>
      auth.unlockWithStoredMasterKey();

  Future<AccountDto?> restoreSession() => auth.restoreSession();

  Future<AccountDto> currentAccount() => auth.currentAccount();

  Future<bool> hasStoredSession() => auth.hasStoredSession();

  Future<bool> isUnlocked() => auth.isUnlocked();

  Future<bool> isKeepUnlocked() => auth.isKeepUnlocked();

  Future<void> setKeepUnlocked(bool enabled) =>
      auth.setKeepUnlocked(enabled: enabled);

  Future<void> lock() => auth.lock();

  Future<void> logout() => auth.logout();

  Future<void> forgetCredentials() => auth.forgetCredentials();

  // ---- 文件 ----
  Future<List<NodeDto>> listFolder(String folderId, NodeSortDto sort) =>
      files.listFolder(folderId: folderId, sort: sort);

  Future<NodeDto> createFolder(String parentId, String name) =>
      files.createFolder(parentId: parentId, name: name);

  Future<NodeDto> renameNode({
    required String nodeId,
    required String parentId,
    required String newName,
    required int rowVersion,
  }) {
    return files.renameNode(
      nodeId: nodeId,
      parentId: parentId,
      newName: newName,
      rowVersion: rowVersion,
    );
  }

  Future<NodeDto> moveNode({
    required String nodeId,
    required String currentParentId,
    required String newParentId,
    required String name,
    required int rowVersion,
  }) {
    return files.moveNode(
      nodeId: nodeId,
      currentParentId: currentParentId,
      newParentId: newParentId,
      name: name,
      rowVersion: rowVersion,
    );
  }

  Future<String> deleteToTrash(String nodeId) =>
      files.deleteToTrash(nodeId: nodeId);

  Future<List<TrashItemDto>> listTrash() => files.listTrash();

  Future<NodeDto> restoreFromTrash(String nodeId, RestoreTargetDto? target) =>
      files.restoreFromTrash(nodeId: nodeId, target: target);

  Future<PermanentDeleteOutcomeDto> permanentlyDelete(String nodeId) =>
      files.permanentlyDelete(nodeId: nodeId);

  Future<List<VersionDto>> listVersions(String nodeId) =>
      files.listVersions(nodeId: nodeId);

  Future<VersionDto> restoreVersion(String nodeId, String sourceVersionId) =>
      files.restoreVersion(nodeId: nodeId, sourceVersionId: sourceVersionId);

  Future<UsageDto> getUsage() => files.getUsage();

  // ---- 传输 ----
  Future<String> startUploadFile(String parentId, String localPath) =>
      transfer.startUploadFile(parentId: parentId, localPath: localPath);

  Future<String> startUploadNewVersion(String nodeId, String localPath) =>
      transfer.startUploadNewVersion(nodeId: nodeId, localPath: localPath);

  Future<String> startUploadFolder(String parentId, String localDir) =>
      transfer.startUploadFolder(parentId: parentId, localDir: localDir);

  Future<String> startDownloadFile({
    required String nodeId,
    required String versionId,
    required String fileName,
    required String destinationDir,
    required ConflictPolicyDto policy,
  }) {
    return transfer.startDownloadFile(
      nodeId: nodeId,
      versionId: versionId,
      fileName: fileName,
      destinationDir: destinationDir,
      policy: policy,
    );
  }

  Future<String> startDownloadFolder({
    required String folderId,
    required String folderName,
    required String destinationRoot,
    required ConflictPolicyDto policy,
  }) {
    return transfer.startDownloadFolder(
      folderId: folderId,
      folderName: folderName,
      destinationRoot: destinationRoot,
      policy: policy,
    );
  }

  Future<List<PendingUploadDto>> listPendingUploads() =>
      transfer.listPendingUploads();

  Future<String> resumePendingUpload(String sessionId) =>
      transfer.resumePendingUpload(sessionId: sessionId);

  Future<void> abortPendingUpload(String sessionId) =>
      transfer.abortPendingUpload(sessionId: sessionId);

  Future<void> pauseTransfer(String transferId) =>
      transfer.pauseTransfer(transferId: transferId);

  Future<void> resumeTransfer(String transferId) =>
      transfer.resumeTransfer(transferId: transferId);

  Future<void> cancelTransfer(String transferId) =>
      transfer.cancelTransfer(transferId: transferId);

  Future<void> retryTransfer(String transferId) =>
      transfer.retryTransfer(transferId: transferId);

  Future<TransferResultDto?> transferResult(String transferId) =>
      transfer.transferResult(transferId: transferId);

  Stream<TransferProgressDto> transferProgressStream() =>
      transfer.transferProgressStream();

  Stream<FolderProgressDto> folderProgressStream() =>
      transfer.folderProgressStream();

  // ---- 备份引擎 ----
  Future<void> backupStart() => backup.backupStart();

  Future<void> backupStop() => backup.backupStop();

  Future<List<BackupFolderDto>> backupListFolders() =>
      backup.backupListFolders();

  Future<BackupFolderDto> backupFolder(String folderId) =>
      backup.backupFolder(folderId: folderId);

  Future<BackupFolderDto> backupAddFolder(BackupFolderDraftDto draft) =>
      backup.backupAddFolder(draft: draft);

  Future<BackupFolderDto> backupUpdateFolder(
    String folderId,
    BackupFolderUpdateDto update,
  ) {
    return backup.backupUpdateFolder(folderId: folderId, update: update);
  }

  Future<void> backupRemoveFolder(String folderId) =>
      backup.backupRemoveFolder(folderId: folderId);

  Future<BackupPreflightDto> backupPreflight(BackupFolderDraftDto draft) =>
      backup.backupPreflight(draft: draft);

  Future<void> backupPauseAll() => backup.backupPauseAll();

  Future<void> backupResumeAll() => backup.backupResumeAll();

  Future<void> backupPauseFolder(String folderId) =>
      backup.backupPauseFolder(folderId: folderId);

  Future<void> backupResumeFolder(String folderId) =>
      backup.backupResumeFolder(folderId: folderId);

  Future<BackupSettingsDto> backupSettings() => backup.backupSettings();

  Future<void> backupSetSettings(BackupSettingsDto settings) =>
      backup.backupSetSettings(settings: settings);

  Future<List<BackupFolderStateDto>> backupFolderStates() =>
      backup.backupFolderStates();

  Future<BackupSummaryDto> backupSummary() => backup.backupSummary();

  Future<BackupSettingsDto> backupCachedSettings() =>
      backup.backupCachedSettings();

  Future<List<BackupActivityDto>> backupRecentActivity(int limit) =>
      backup.backupRecentActivity(limit: limit);

  Future<BackupRunReportDto> backupScanFolderNow(String folderId) =>
      backup.backupScanFolderNow(folderId: folderId);

  Future<List<BackupRunReportDto>> backupScanAllNow() =>
      backup.backupScanAllNow();

  Stream<BackupEventDto> backupEventsStream() => backup.backupEventsStream();

  // ---- 微信备份 ----
  /// 发现本机微信账号（展示字段 + 本地备注名）。
  Future<List<WechatAccountDto>> wechatListAccounts() =>
      wechat.wechatListAccounts();

  /// 手动添加微信数据目录；返回重新发现后的账号列表。
  Future<List<WechatAccountDto>> wechatAddManualDirectory(String path) =>
      wechat.wechatAddManualDirectory(path: path);

  /// 移除手动添加的微信数据目录；返回重新发现后的账号列表。
  Future<List<WechatAccountDto>> wechatRemoveManualDirectory(String path) =>
      wechat.wechatRemoveManualDirectory(path: path);

  /// 当前手动添加的微信数据目录。
  Future<List<String>> wechatManualDirectories() =>
      wechat.wechatManualDirectories();

  /// 设置账号备注名（只存本地）。
  Future<void> wechatSetNote(WechatAccountRefDto account, String note) =>
      wechat.wechatSetNote(account: account, note: note);

  /// 清除账号备注名。
  Future<void> wechatClearNote(WechatAccountRefDto account) =>
      wechat.wechatClearNote(account: account);

  /// 设置一个账号勾选的类别。
  Future<void> wechatSetCategories(
    WechatAccountRefDto account,
    List<WechatCategoryDto> categories,
  ) {
    return wechat.wechatSetCategories(account: account, categories: categories);
  }

  /// 微信备份首次上传预检。
  Future<WechatBackupPreflightDto> wechatPreflight(
    WechatAccountRefDto account,
    List<WechatCategoryDto> categories,
  ) {
    return wechat.wechatPreflight(account: account, categories: categories);
  }

  /// 每个微信账号每个类别的备份状态。
  Future<List<WechatAccountBackupStatusDto>> wechatAccountStatuses() =>
      wechat.wechatAccountStatuses();

  /// 立即为一个账号做数据库快照。
  Future<WechatSnapshotReportDto> wechatSnapshotNow(
    WechatAccountRefDto account,
  ) {
    return wechat.wechatSnapshotNow(account: account);
  }

  /// 列出某账号可用的完整数据库快照。
  Future<List<WechatDatabaseSnapshotDto>> wechatDatabaseSnapshots(
    WechatAccountRefDto account,
  ) {
    return wechat.wechatDatabaseSnapshots(account: account);
  }

  /// 列出云端存在哪些文件类别可恢复。
  Future<List<WechatRestoreCategoryDto>> wechatRestoreCategories(
    WechatAccountRefDto account,
  ) {
    return wechat.wechatRestoreCategories(account: account);
  }

  /// 开始恢复；返回传输 ID，进度通过 [transferProgressStream] 推送。
  Future<String> wechatStartRestore(WechatRestoreRequestDto request) =>
      wechat.wechatStartRestore(request: request);

  // ---- 客户端更新 ----
  /// 更新下载进度（含已下载 / 总字节 / 速度；终态为 completed/failed/cancelled）。
  Stream<UpdateProgressDto> updateProgressStream() =>
      update.updateProgressStream();

  /// 检查更新；`now` 是 Unix 秒（0 表示使用 Rust 侧时间）。
  Future<UpdateCheckDto> updateCheck(int now) => update.updateCheck(now: now);

  Future<String?> updateBannerDismissedVersion() =>
      update.updateBannerDismissedVersion();

  Future<void> updateDismissBanner(String version) =>
      update.updateDismissBanner(version: version);

  Future<String> updateDownload() => update.updateDownload();

  Future<void> updateCancelDownload() => update.updateCancelDownload();

  Future<UpdateInstallOutcomeDto> updateInstall() => update.updateInstall();

  Future<void> updateOpenDmg() => update.updateOpenDmg();
}
