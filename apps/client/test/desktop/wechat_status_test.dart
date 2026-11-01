import 'package:encrypted_backup_client/core/rust/models.dart';
import 'package:encrypted_backup_client/desktop/state/wechat_status.dart';
import 'package:flutter_test/flutter_test.dart';

WechatCategoryBackupStatusDto _status(
  WechatCategoryDto category, {
  bool selected = true,
  BackupFolderStatusDto? status,
}) {
  return WechatCategoryBackupStatusDto(
    category: category,
    selected: selected,
    status: status,
    pauseReason: null,
    lastBackupAt: null,
  );
}

void main() {
  test('类别状态映射覆盖等待、上传、暂停与无内容', () {
    const info = WechatCategoryInfoDto(
      category: WechatCategoryDto.databases,
      sizeBytes: 100,
      fileCount: 1,
      lastModifiedNs: null,
      exists: true,
    );
    final waiting = wechatCategoryStatusView(
      category: WechatCategoryDto.databases,
      info: info,
      status: _status(
        WechatCategoryDto.databases,
        status: const BackupFolderStatusDto(
          kind: BackupFolderStatusKindDto.waitingForWechatExit,
          done: 0,
          total: 0,
          unreadableCount: 0,
        ),
      ),
    );
    expect(waiting.label, '等微信退出后备份');

    final uploading = wechatCategoryStatusView(
      category: WechatCategoryDto.databases,
      info: info,
      status: _status(
        WechatCategoryDto.databases,
        status: const BackupFolderStatusDto(
          kind: BackupFolderStatusKindDto.uploading,
          done: 12,
          total: 30,
          unreadableCount: 0,
        ),
      ),
    );
    expect(uploading.label, '正在备份 12 / 30');

    final paused = wechatCategoryStatusView(
      category: WechatCategoryDto.databases,
      info: info,
      status: _status(
        WechatCategoryDto.databases,
        status: const BackupFolderStatusDto(
          kind: BackupFolderStatusKindDto.paused,
          done: 0,
          total: 0,
          unreadableCount: 0,
        ),
      ),
    );
    expect(paused.label, '暂停');

    final empty = wechatCategoryStatusView(
      category: WechatCategoryDto.databases,
      info: const WechatCategoryInfoDto(
        category: WechatCategoryDto.databases,
        sizeBytes: 0,
        fileCount: 0,
        lastModifiedNs: null,
        exists: false,
      ),
      status: null,
    );
    expect(empty.label, '无内容');
  });

  test('微信来源事件显示为“微信备份 · 类别”', () {
    expect(
      wechatEventSourceLabel(
        const WechatEventSourceDto(
          accountName: 'wxid_test',
          category: WechatCategoryDto.chatFiles,
        ),
      ),
      '微信备份 · 聊天文件',
    );
  });
}
