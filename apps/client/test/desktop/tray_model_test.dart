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

  test('有新版本时托盘菜单显示可点击的版本项', () {
    final model = trayMenuModelFor(
      _summary(BackupSummaryStatusKindDto.allBackedUp),
      update: const TrayUpdateEntry.available('0.2.0'),
    );
    expect(model.update?.label, '有新版本 0.2.0…');
    expect(model.update?.enabled, isTrue);
  });

  test('下载中托盘菜单显示百分比并禁用', () {
    final model = trayMenuModelFor(
      _summary(BackupSummaryStatusKindDto.allBackedUp),
      update: const TrayUpdateEntry.downloading(62),
    );
    expect(model.update?.label, '正在下载更新 62%');
    expect(model.update?.enabled, isFalse);
  });

  test('强制更新时禁用暂停/恢复并提示需要更新', () {
    final model = trayMenuModelFor(
      _summary(BackupSummaryStatusKindDto.allBackedUp),
      backupBlocked: true,
    );
    expect(model.pauseResumeEnabled, isFalse);
    expect(model.statusLabel, '需要更新后才能继续使用');
    expect(model.update, isNull);
  });

  test('没有更新时不生成托盘更新项', () {
    final model = trayMenuModelFor(
      _summary(BackupSummaryStatusKindDto.allBackedUp),
    );
    expect(model.update, isNull);
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
