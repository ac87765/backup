import 'package:encrypted_backup_client/core/rust/models.dart';
import 'package:encrypted_backup_client/desktop/state/backup_folder_status.dart';
import 'package:flutter_test/flutter_test.dart';

BackupFolderDto _folder({
  String id = 'folder-1',
  bool enabled = true,
  String name = '工作资料',
}) {
  return BackupFolderDto(
    id: id,
    localPath: 'D:\\$name',
    localName: name,
    cloudTargetId: null,
    cloudFolderId: 'cloud-1',
    cloudPath: '全部文件 / 电脑备份 / DESKTOP-TLJ / $name',
    excludeGlobs: const <String>[],
    excludeSubfolders: const <String>[],
    enabled: enabled,
    lastBackupAt: 1761900000,
  );
}

BackupFolderStateDto _state({
  BackupFolderStatusKindDto kind = BackupFolderStatusKindDto.idleOk,
  int done = 0,
  int total = 0,
  int unreadable = 0,
  BackupPauseReasonDto? pauseReason,
}) {
  return BackupFolderStateDto(
    folderId: 'folder-1',
    status: BackupFolderStatusDto(
      kind: kind,
      done: done,
      total: total,
      unreadableCount: unreadable,
    ),
    pauseReason: pauseReason,
    lastBackupAt: 1761900000,
    wechat: null,
  );
}

void main() {
  test('五种卡片状态映射', () {
    final ok = folderStatusView(_folder(), _state(), globalPaused: false);
    expect(ok.label, '已备份');
    expect(ok.tone, FolderStatusTone.ok);

    final busy = folderStatusView(
      _folder(),
      _state(kind: BackupFolderStatusKindDto.uploading, done: 128, total: 340),
      globalPaused: false,
    );
    expect(busy.label, '正在备份 128 / 340');
    expect(busy.tone, FolderStatusTone.busy);
    expect(busy.progress, closeTo(128 / 340, 0.0001));

    final paused = folderStatusView(
      _folder(),
      _state(kind: BackupFolderStatusKindDto.paused),
      globalPaused: false,
    );
    expect(paused.label, '已暂停');
    expect(paused.tone, FolderStatusTone.paused);

    final unreadable = folderStatusView(
      _folder(),
      _state(kind: BackupFolderStatusKindDto.error, unreadable: 2),
      globalPaused: false,
    );
    expect(unreadable.label, '2 个文件无法读取');
    expect(unreadable.tone, FolderStatusTone.warn);

    final locked = folderStatusView(
      _folder(),
      _state(kind: BackupFolderStatusKindDto.needsUnlock),
      globalPaused: false,
    );
    expect(locked.label, '需要解锁');
    expect(locked.tone, FolderStatusTone.needsUnlock);
  });

  test('全局暂停时显示剩余数量', () {
    final status = folderStatusView(
      _folder(),
      _state(kind: BackupFolderStatusKindDto.uploading, done: 128, total: 340),
      globalPaused: true,
    );
    expect(status.label, '已暂停 · 剩余 212 个');
    expect(status.progress, isNull);
  });

  test('账号未解锁时所有卡片显示需要解锁', () {
    final status = folderStatusView(
      _folder(),
      _state(),
      globalPaused: false,
      needsUnlock: true,
    );
    expect(status.tone, FolderStatusTone.needsUnlock);
  });

  test('整个文件夹位于微信数据目录内时由微信备份处理', () {
    final status = folderStatusView(
      _folder(),
      _state(),
      globalPaused: false,
      managedByWechat: true,
    );
    expect(status.label, '由微信备份处理');
    expect(status.tone, FolderStatusTone.wechat);
  });

  test('文件夹被单独暂停时显示已暂停', () {
    final status = folderStatusView(
      _folder(enabled: false),
      _state(),
      globalPaused: false,
    );
    expect(status.label, '已暂停');
  });
}
