import 'package:encrypted_backup_client/core/rust/models.dart';
import 'package:encrypted_backup_client/desktop/tray/tray_model.dart';
import 'package:flutter_test/flutter_test.dart';

BackupSummaryDto _summary(
  BackupSummaryStatusKindDto kind, {
  int done = 0,
  int total = 0,
}) {
  return BackupSummaryDto(
    kind: kind,
    done: done,
    total: total,
    unreadableCount: 0,
    folderCount: 3,
    lastCheckedAt: null,
  );
}

void main() {
  test('已全部备份 → 正常图标与暂停菜单', () {
    final model = trayMenuModelFor(
      _summary(BackupSummaryStatusKindDto.allBackedUp),
    );
    expect(model.icon, TrayIconState.normal);
    expect(model.statusLabel, '已全部备份');
    expect(model.pauseResumeLabel, '暂停备份');
    expect(model.pauseResumeEnabled, isTrue);
    expect(model.needsUnlock, isFalse);
  });

  test('正在备份 → 上传图标与进度文字', () {
    final model = trayMenuModelFor(
      _summary(BackupSummaryStatusKindDto.uploading, done: 3, total: 10),
    );
    expect(model.icon, TrayIconState.uploading);
    expect(model.statusLabel, '正在备份 3 / 10');
  });

  test('已暂停 → 暂停图标与恢复菜单', () {
    final model = trayMenuModelFor(_summary(BackupSummaryStatusKindDto.paused));
    expect(model.icon, TrayIconState.paused);
    expect(model.statusLabel, '已暂停');
    expect(model.pauseResumeLabel, '恢复备份');
  });

  test('需要解锁 → 解锁图标且不能暂停/恢复', () {
    final model = trayMenuModelFor(
      _summary(BackupSummaryStatusKindDto.needsUnlock),
    );
    expect(model.icon, TrayIconState.needsUnlock);
    expect(model.statusLabel, '需要解锁才能继续备份');
    expect(model.needsUnlock, isTrue);
    expect(model.pauseResumeEnabled, isFalse);
    expect(model.tooltip, '需要解锁才能继续备份');
  });

  test('认证阶段未解锁覆盖备份汇总', () {
    final model = trayMenuModelFor(
      _summary(BackupSummaryStatusKindDto.allBackedUp),
      needsUnlock: true,
    );
    expect(model.icon, TrayIconState.needsUnlock);
    expect(model.needsUnlock, isTrue);
  });

  test('备份遇到问题时复用暂停图标', () {
    final model = trayMenuModelFor(_summary(BackupSummaryStatusKindDto.error));
    expect(model.icon, TrayIconState.paused);
    expect(model.statusLabel, '备份遇到问题');
  });

  test('图标资源路径', () {
    expect(
      trayIconAsset(TrayIconState.needsUnlock, macOS: false),
      'assets/tray/tray-needs-unlock.ico',
    );
    expect(
      trayIconAsset(TrayIconState.uploading, macOS: true),
      'assets/tray/tray-uploadingTemplate.png',
    );
  });
}
