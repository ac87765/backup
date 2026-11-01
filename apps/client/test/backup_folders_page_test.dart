import 'package:encrypted_backup_client/common/widgets/controls.dart';
import 'package:encrypted_backup_client/core/rust/models.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_native_bridge.dart';
import 'support/test_harness.dart';

BackupFolderDto _folder({
  required String id,
  required String name,
  String localPath = r'D:\工作资料',
  bool enabled = true,
}) {
  return BackupFolderDto(
    id: id,
    localPath: localPath,
    localName: name,
    cloudTargetId: null,
    cloudFolderId: 'cloud-$id',
    cloudPath: '全部文件 / 电脑备份 / DESKTOP-TLJ / $name',
    excludeGlobs: const <String>[],
    excludeSubfolders: const <String>[],
    enabled: enabled,
    lastBackupAt: 1761900000,
  );
}

BackupFolderStateDto _state(
  String id, {
  BackupFolderStatusKindDto kind = BackupFolderStatusKindDto.idleOk,
  int done = 0,
  int total = 0,
  int unreadable = 0,
  BackupPauseReasonDto? pauseReason,
}) {
  return BackupFolderStateDto(
    folderId: id,
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

Future<void> _openBackupPage(WidgetTester tester, TestHarness harness) async {
  await pumpApp(tester, harness);
  await tester.tap(find.text('备份文件夹').first);
  await tester.pumpAndSettle();
}

void main() {
  testWidgets('五种卡片状态与最近活动', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final bridge = FakeNativeBridge(
      account: testAccount(),
      storedSession: true,
    );
    bridge.backupFolders = <BackupFolderDto>[
      _folder(id: 'f1', name: '工作资料'),
      _folder(id: 'f2', name: '照片', localPath: r'C:\Users\tlj\Pictures'),
      _folder(id: 'f3', name: '文档', localPath: r'C:\Users\tlj\Documents'),
      _folder(id: 'f4', name: '项目', localPath: r'D:\项目'),
      _folder(id: 'f5', name: '归档', localPath: r'D:\归档'),
    ];
    bridge.backupStates = <BackupFolderStateDto>[
      _state('f1'),
      _state(
        'f2',
        kind: BackupFolderStatusKindDto.uploading,
        done: 128,
        total: 340,
      ),
      _state(
        'f3',
        kind: BackupFolderStatusKindDto.paused,
        pauseReason: BackupPauseReasonDto.user,
      ),
      _state('f4', kind: BackupFolderStatusKindDto.error, unreadable: 2),
      _state('f5', kind: BackupFolderStatusKindDto.needsUnlock),
    ];
    bridge.backupActivities = <BackupActivityDto>[
      const BackupActivityDto(
        id: 1,
        folderId: 'f2',
        relativePath: 'IMG_2041.HEIC',
        kind: BackupActivityKindDto.added,
        occurredAt: 1761900300,
        wechat: null,
      ),
      const BackupActivityDto(
        id: 2,
        folderId: 'f1',
        relativePath: 'Q3 预算.xlsx',
        kind: BackupActivityKindDto.newVersion,
        occurredAt: 1761900000,
        wechat: null,
      ),
      const BackupActivityDto(
        id: 3,
        folderId: 'f3',
        relativePath: r'~$周报.docx',
        kind: BackupActivityKindDto.retryLater,
        occurredAt: 1761890000,
        wechat: null,
      ),
    ];
    final harness = TestHarness(bridge: bridge);
    await _openBackupPage(tester, harness);

    expect(find.text('已备份'), findsOneWidget);
    expect(find.text('正在备份 128 / 340'), findsOneWidget);
    expect(find.text('已暂停'), findsOneWidget);
    expect(find.text('2 个文件无法读取'), findsOneWidget);
    expect(find.text('需要解锁'), findsOneWidget);
    expect(find.text('IMG_2041.HEIC'), findsOneWidget);
    expect(find.text('被其他程序占用，稍后重试'), findsOneWidget);
    expect(find.text('最近活动'), findsOneWidget);
  });

  testWidgets('暂停与恢复备份横幅', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final bridge = FakeNativeBridge(
      account: testAccount(),
      storedSession: true,
    );
    bridge.backupFolders = <BackupFolderDto>[_folder(id: 'f1', name: '工作资料')];
    bridge.backupStates = <BackupFolderStateDto>[_state('f1')];
    final harness = TestHarness(bridge: bridge);
    await _openBackupPage(tester, harness);

    await tester.tap(find.byKey(const Key('backup-pause-all')));
    await tester.pumpAndSettle();

    expect(harness.bridge.calls, contains('backupPauseAll'));
    expect(find.byKey(const Key('backup-paused-banner')), findsOneWidget);
    expect(find.text('备份已暂停。新的改动会在恢复后一起备份。'), findsOneWidget);
    expect(find.byKey(const Key('backup-resume-all')), findsOneWidget);

    await tester.tap(find.byKey(const Key('backup-resume-all')));
    await tester.pumpAndSettle();
    expect(harness.bridge.calls, contains('backupResumeAll'));
    expect(find.byKey(const Key('backup-paused-banner')), findsNothing);
  });

  testWidgets('添加备份文件夹：预检、排除规则与开始备份', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final bridge = FakeNativeBridge(
      account: testAccount(),
      storedSession: true,
    );
    final harness = TestHarness(bridge: bridge);
    harness.filePicker.directoryToPick = r'D:\工作资料';
    await pumpApp(tester, harness);
    await tester.tap(find.text('备份文件夹').first);
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const Key('backup-add-folder')));
    await tester.pumpAndSettle();
    expect(find.text('添加备份文件夹'), findsOneWidget);

    await tester.tap(find.byKey(const Key('backup-pick-folder')));
    await tester.pumpAndSettle();

    expect(harness.bridge.calls, contains(r'backupPreflight:D:\工作资料'));
    expect(find.text('1,284'), findsOneWidget);
    expect(find.text('3.2 GB'), findsOneWidget);
    expect(find.text('37.6 GB'), findsOneWidget);
    expect(find.byKey(const Key('exclude-fixed-rule')), findsOneWidget);
    expect(
      find.byKey(const ValueKey<String>('exclude-rule-remove-系统与临时文件')),
      findsNothing,
    );
    expect(
      find.byKey(const ValueKey<String>('subfolder-toggle-合同')),
      findsOneWidget,
    );

    await tester.tap(find.byKey(const Key('backup-start')));
    await tester.pumpAndSettle();

    expect(harness.bridge.calls, contains(r'backupAddFolder:D:\工作资料'));
    expect(find.text('添加备份文件夹'), findsNothing);
    expect(find.text('工作资料'), findsOneWidget);
  });

  testWidgets('配额不足时开始按钮禁用并显示警告', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final bridge = FakeNativeBridge(
      account: testAccount(),
      storedSession: true,
    );
    bridge.preflight = const BackupPreflightDto(
      fileCount: 18902,
      totalBytes: 52800000000,
      uploadBytes: 52800000000,
      excludedCount: 0,
      unreadableCount: 0,
      remainingQuotaBytes: 37600000000,
    );
    final harness = TestHarness(bridge: bridge);
    harness.filePicker.directoryToPick = r'D:\工作资料';
    await pumpApp(tester, harness);
    await tester.tap(find.text('备份文件夹').first);
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const Key('backup-add-folder')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('backup-pick-folder')));
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('backup-quota-warning')), findsOneWidget);
    final start = tester.widget<TokensButton>(
      find.byKey(const Key('backup-start')),
    );
    expect(start.onPressed, isNull);
  });

  testWidgets('添加排除规则', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final bridge = FakeNativeBridge(
      account: testAccount(),
      storedSession: true,
    );
    final harness = TestHarness(bridge: bridge);
    harness.filePicker.directoryToPick = r'D:\工作资料';
    await pumpApp(tester, harness);
    await tester.tap(find.text('备份文件夹').first);
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('backup-add-folder')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('backup-pick-folder')));
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const Key('exclude-rule-add')));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byKey(const Key('exclude-rule-input')),
      '*.psd',
    );
    await tester.tap(find.byKey(const Key('exclude-rule-confirm')));
    await tester.pumpAndSettle();

    expect(find.text('*.psd'), findsOneWidget);
  });

  testWidgets('移除前确认并说明云端文件保留', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final bridge = FakeNativeBridge(
      account: testAccount(),
      storedSession: true,
    );
    bridge.backupFolders = <BackupFolderDto>[_folder(id: 'f1', name: '工作资料')];
    bridge.backupStates = <BackupFolderStateDto>[_state('f1')];
    final harness = TestHarness(bridge: bridge);
    await _openBackupPage(tester, harness);

    await tester.tap(
      find.byKey(const ValueKey<String>('backup-folder-more-f1')),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey<String>('menu-item-移除')));
    await tester.pumpAndSettle();

    expect(find.text('移除备份文件夹？'), findsOneWidget);
    expect(find.textContaining('云端已备份的文件会保留'), findsOneWidget);

    await tester.tap(find.text('移除').last);
    await tester.pumpAndSettle();

    expect(harness.bridge.calls, contains('backupRemoveFolder:f1'));
    expect(find.text('让电脑上的文件自动备份'), findsOneWidget);
  });
}
