import 'package:encrypted_backup_client/common/widgets/controls.dart';
import 'package:encrypted_backup_client/core/rust/models.dart';
import 'package:encrypted_backup_client/desktop/pages/wechat_page.dart';
import 'package:encrypted_backup_client/desktop/state/wechat_controller.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/fake_native_bridge.dart';
import '../support/test_harness.dart';

const _root = r'C:\Users\tlj\Documents\xwechat_files';
const _accountName = 'wxid_k8f3abc27';

WechatCategoryInfoDto _info(
  WechatCategoryDto category, {
  int bytes = 0,
  bool exists = true,
}) {
  return WechatCategoryInfoDto(
    category: category,
    sizeBytes: bytes,
    fileCount: exists ? 10 : 0,
    lastModifiedNs: exists ? 1761900000000000000 : null,
    exists: exists,
  );
}

WechatAccountDto _account({
  String accountName = _accountName,
  String? note,
  bool migration = false,
  bool current = true,
  int totalSizeBytes = 3100000000,
  List<WechatCategoryInfoDto>? categories,
}) {
  return WechatAccountDto(
    accountName: accountName,
    version: WechatVersionDto.wechat4,
    dataRoot: _root,
    accountDir: '$_root\\$accountName',
    source: WechatSourceDto.defaultLocation,
    categories:
        categories ??
        <WechatCategoryInfoDto>[
          _info(WechatCategoryDto.chatFiles, bytes: 205000000),
          _info(WechatCategoryDto.imagesAndVideos, bytes: 2400000000),
          _info(WechatCategoryDto.databases, bytes: 163000000),
          _info(
            WechatCategoryDto.phoneMigration,
            exists: migration,
            bytes: migration ? 400000000 : 0,
          ),
        ],
    totalSizeBytes: totalSizeBytes,
    totalFileCount: 1000,
    lastModifiedNs: 1761900000000000000,
    isCurrentLoginCandidate: current,
    hasPhoneMigrationBackup: migration,
    note: note,
  );
}

BackupFolderStatusDto _folderStatus(
  BackupFolderStatusKindDto kind, {
  int done = 0,
  int total = 0,
  int unreadable = 0,
}) {
  return BackupFolderStatusDto(
    kind: kind,
    done: done,
    total: total,
    unreadableCount: unreadable,
  );
}

WechatAccountBackupStatusDto _accountStatus(
  WechatAccountDto account, {
  Set<WechatCategoryDto> selected = const <WechatCategoryDto>{
    WechatCategoryDto.chatFiles,
    WechatCategoryDto.imagesAndVideos,
    WechatCategoryDto.databases,
  },
  Map<WechatCategoryDto, BackupFolderStatusDto> statuses =
      const <WechatCategoryDto, BackupFolderStatusDto>{},
  Map<WechatCategoryDto, int> lastBackups = const <WechatCategoryDto, int>{},
}) {
  return WechatAccountBackupStatusDto(
    dataRoot: account.dataRoot,
    accountName: account.accountName,
    version: account.version,
    categories: <WechatCategoryBackupStatusDto>[
      for (final category in WechatCategoryDto.values)
        WechatCategoryBackupStatusDto(
          category: category,
          selected: selected.contains(category),
          status: statuses[category],
          pauseReason: null,
          lastBackupAt: lastBackups[category],
        ),
    ],
    lastBackupAt: lastBackups.values.isEmpty
        ? null
        : lastBackups.values.reduce(
            (left, right) => left > right ? left : right,
          ),
  );
}

Future<void> _openWechat(WidgetTester tester, TestHarness harness) async {
  await pumpApp(tester, harness);
  await tester.tap(find.text('微信备份').first);
  await tester.pumpAndSettle();
}

ProviderContainer _container(WidgetTester tester) {
  return ProviderScope.containerOf(
    tester.element(find.byType(WechatBackupPage)),
    listen: false,
  );
}

void main() {
  testWidgets('notfound → 手动选择目录后出现账号', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final bridge = FakeNativeBridge(
      account: testAccount(),
      storedSession: true,
    );
    bridge.wechatAccountsAfterManualAdd = <WechatAccountDto>[_account()];
    final harness = TestHarness(bridge: bridge);
    harness.filePicker.directoryToPick = r'D:\WeChatData';
    await _openWechat(tester, harness);

    expect(find.text('没有找到微信数据'), findsOneWidget);
    await tester.tap(find.byKey(const Key('wechat-pick-directory')));
    await tester.pumpAndSettle();

    expect(
      harness.bridge.calls,
      contains(r'wechatAddManualDirectory:D:\WeChatData'),
    );
    expect(find.byKey(const Key('wechat-setup-card')), findsOneWidget);
    expect(find.text('找到 1 个微信账号'), findsOneWidget);
  });

  testWidgets('首次开启默认勾选与预检数字', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final bridge = FakeNativeBridge(
      account: testAccount(),
      storedSession: true,
    );
    bridge.wechatAccounts = <WechatAccountDto>[_account()];
    bridge.wechatPreflightValue = const WechatBackupPreflightDto(
      fileCount: 1284,
      totalBytes: 2800000000,
      uploadBytes: 2800000000,
      uploadBytesWithoutImagesAndVideos: 400000000,
      databaseBytes: 163000000,
      excludedCount: 0,
      unreadableCount: 0,
      remainingQuotaBytes: 37600000000,
      overQuota: false,
    );
    final harness = TestHarness(bridge: bridge);
    await _openWechat(tester, harness);

    final state = _container(tester).read(wechatControllerProvider);
    expect(
      state.setupCategories,
      containsAll(<WechatCategoryDto>[
        WechatCategoryDto.chatFiles,
        WechatCategoryDto.imagesAndVideos,
        WechatCategoryDto.databases,
      ]),
    );
    expect(
      state.setupCategories,
      isNot(contains(WechatCategoryDto.phoneMigration)),
    );
    expect(find.text('2.8 GB'), findsOneWidget);
    expect(find.text('37.6 GB'), findsOneWidget);
    expect(harness.bridge.calls, contains('wechatPreflight:$_accountName'));

    final start = tester.widget<TokensButton>(
      find.byKey(const Key('wechat-start-backup')),
    );
    expect(start.onPressed, isNotNull);

    await tester.tap(find.byKey(const Key('wechat-start-backup')));
    await tester.pumpAndSettle();
    expect(harness.bridge.calls, contains('wechatSetCategories:$_accountName'));
    expect(
      find.byKey(const ValueKey<String>('wechat-account-$_accountName')),
      findsOneWidget,
    );
  });

  testWidgets('quota 禁用开启按钮，取消图片与视频后恢复可用', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final bridge = FakeNativeBridge(
      account: testAccount(),
      storedSession: true,
    );
    bridge.wechatAccounts = <WechatAccountDto>[_account()];
    bridge.wechatPreflightBuilder = (account, categories) {
      final withImages = categories.contains(WechatCategoryDto.imagesAndVideos);
      return WechatBackupPreflightDto(
        fileCount: withImages ? 1284 : 400,
        totalBytes: withImages ? 2800000000 : 400000000,
        uploadBytes: withImages ? 2800000000 : 400000000,
        uploadBytesWithoutImagesAndVideos: 400000000,
        databaseBytes: 163000000,
        excludedCount: 0,
        unreadableCount: 0,
        remainingQuotaBytes: 1900000000,
        overQuota: withImages,
      );
    };
    final harness = TestHarness(bridge: bridge);
    await _openWechat(tester, harness);

    expect(find.byKey(const Key('wechat-quota-warning')), findsOneWidget);
    var start = tester.widget<TokensButton>(
      find.byKey(const Key('wechat-start-backup')),
    );
    expect(start.onPressed, isNull);

    await tester.tap(
      find.byKey(
        const ValueKey<String>('wechat-setup-category-imagesAndVideos'),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('wechat-quota-warning')), findsNothing);
    start = tester.widget<TokensButton>(
      find.byKey(const Key('wechat-start-backup')),
    );
    expect(start.onPressed, isNotNull);
  });

  testWidgets('default 类别状态映射（含等微信退出后备份）', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final bridge = FakeNativeBridge(
      account: testAccount(),
      storedSession: true,
    );
    final first = _account();
    final second = _account(
      accountName: 'wxid_q2m9def61',
      current: false,
      totalSizeBytes: 88000000,
      categories: <WechatCategoryInfoDto>[
        _info(WechatCategoryDto.chatFiles, bytes: 400000),
        _info(WechatCategoryDto.imagesAndVideos, exists: false),
        _info(WechatCategoryDto.databases, bytes: 1000000),
        _info(WechatCategoryDto.phoneMigration, exists: false),
      ],
    );
    bridge.wechatAccounts = <WechatAccountDto>[first, second];
    bridge.wechatStatuses = <WechatAccountBackupStatusDto>[
      _accountStatus(
        first,
        statuses: <WechatCategoryDto, BackupFolderStatusDto>{
          WechatCategoryDto.chatFiles: _folderStatus(
            BackupFolderStatusKindDto.idleOk,
          ),
          WechatCategoryDto.imagesAndVideos: _folderStatus(
            BackupFolderStatusKindDto.uploading,
            done: 128,
            total: 940,
          ),
          WechatCategoryDto.databases: _folderStatus(
            BackupFolderStatusKindDto.waitingForWechatExit,
          ),
        },
        lastBackups: <WechatCategoryDto, int>{
          WechatCategoryDto.databases: 1761865800,
        },
      ),
      _accountStatus(
        second,
        selected: <WechatCategoryDto>{
          WechatCategoryDto.chatFiles,
          WechatCategoryDto.databases,
        },
        statuses: <WechatCategoryDto, BackupFolderStatusDto>{
          WechatCategoryDto.chatFiles: _folderStatus(
            BackupFolderStatusKindDto.idleOk,
          ),
          WechatCategoryDto.databases: _folderStatus(
            BackupFolderStatusKindDto.waitingForWechatExit,
          ),
        },
      ),
    ];
    final harness = TestHarness(bridge: bridge);
    await _openWechat(tester, harness);

    expect(find.text('正在备份 128 个文件'), findsOneWidget);
    expect(find.text('正在备份 128 / 940'), findsOneWidget);
    expect(find.text('等微信退出后备份'), findsOneWidget);
    expect(find.text('无内容'), findsOneWidget);
    expect(find.text('已备份'), findsWidgets);

    await tester.tap(
      find.byKey(const ValueKey<String>('wechat-expand-wxid_q2m9def61')),
    );
    await tester.pumpAndSettle();
    expect(
      find.byKey(
        const ValueKey<String>('wechat-category-wxid_q2m9def61-chatFiles'),
      ),
      findsOneWidget,
    );
  });

  testWidgets('修改类别勾选调用 wechatSetCategories', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final bridge = FakeNativeBridge(
      account: testAccount(),
      storedSession: true,
    );
    final account = _account();
    bridge.wechatAccounts = <WechatAccountDto>[account];
    bridge.wechatStatuses = <WechatAccountBackupStatusDto>[
      _accountStatus(
        account,
        statuses: <WechatCategoryDto, BackupFolderStatusDto>{
          WechatCategoryDto.chatFiles: _folderStatus(
            BackupFolderStatusKindDto.idleOk,
          ),
          WechatCategoryDto.imagesAndVideos: _folderStatus(
            BackupFolderStatusKindDto.idleOk,
          ),
          WechatCategoryDto.databases: _folderStatus(
            BackupFolderStatusKindDto.waitingForWechatExit,
          ),
        },
      ),
    ];
    final harness = TestHarness(bridge: bridge);
    await _openWechat(tester, harness);

    await tester.tap(
      find.byKey(
        const ValueKey<String>('wechat-category-wxid_k8f3abc27-chatFiles'),
      ),
    );
    await tester.pumpAndSettle();

    expect(harness.bridge.calls, contains('wechatSetCategories:$_accountName'));
    expect(
      harness.bridge.lastWechatCategories,
      isNot(contains(WechatCategoryDto.chatFiles)),
    );
  });

  testWidgets('设置本地备注名调用 wechatSetNote', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final bridge = FakeNativeBridge(
      account: testAccount(),
      storedSession: true,
    );
    final account = _account();
    bridge.wechatAccounts = <WechatAccountDto>[account];
    bridge.wechatStatuses = <WechatAccountBackupStatusDto>[
      _accountStatus(
        account,
        statuses: <WechatCategoryDto, BackupFolderStatusDto>{
          WechatCategoryDto.chatFiles: _folderStatus(
            BackupFolderStatusKindDto.idleOk,
          ),
        },
      ),
    ];
    final harness = TestHarness(bridge: bridge);
    await _openWechat(tester, harness);

    await tester.tap(
      find.byKey(const ValueKey<String>('wechat-note-$_accountName')),
    );
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const Key('wechat-note-input')), '工作微信');
    await tester.tap(find.byKey(const Key('wechat-note-save')));
    await tester.pumpAndSettle();

    expect(harness.bridge.calls, contains('wechatSetNote:$_accountName'));
    expect(find.text('工作微信'), findsOneWidget);
  });

  testWidgets('恢复对话框只列完整快照；微信运行时提示不关闭；成功后进入传输列表', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final bridge = FakeNativeBridge(
      account: testAccount(),
      storedSession: true,
    );
    final account = _account();
    bridge.wechatAccounts = <WechatAccountDto>[account];
    bridge.wechatStatuses = <WechatAccountBackupStatusDto>[
      _accountStatus(
        account,
        statuses: <WechatCategoryDto, BackupFolderStatusDto>{
          WechatCategoryDto.chatFiles: _folderStatus(
            BackupFolderStatusKindDto.idleOk,
          ),
          WechatCategoryDto.databases: _folderStatus(
            BackupFolderStatusKindDto.idleOk,
          ),
        },
      ),
    ];
    bridge.wechatRestoreCategoryList = const <WechatRestoreCategoryDto>[
      WechatRestoreCategoryDto(
        category: WechatCategoryDto.chatFiles,
        fileCount: 12,
        totalBytes: 205000000,
        available: true,
      ),
    ];
    bridge.wechatSnapshots = <WechatDatabaseSnapshotDto>[
      WechatDatabaseSnapshotDto(
        deviceName: 'DESKTOP-TLJ',
        timestamp: '20261030T231000Z',
        snapshotAt:
            DateTime(2026, 10, 30, 23, 10).millisecondsSinceEpoch ~/ 1000,
        totalBytes: 163000000,
        fileCount: 32,
        complete: true,
      ),
      WechatDatabaseSnapshotDto(
        deviceName: 'DESKTOP-TLJ',
        timestamp: '20261029T120000Z',
        snapshotAt: DateTime(2026, 10, 29, 12).millisecondsSinceEpoch ~/ 1000,
        totalBytes: 158000000,
        fileCount: 31,
        complete: false,
      ),
    ];
    bridge.wechatRestoreError = const BridgeError(
      code: 'wechat_running',
      message: '微信正在运行',
    );
    final harness = TestHarness(bridge: bridge);
    await _openWechat(tester, harness);

    await tester.tap(find.byKey(const Key('wechat-restore')));
    await tester.pumpAndSettle();

    expect(find.text('恢复到微信'), findsOneWidget);
    expect(find.text('昨天 23:10'), findsOneWidget);
    expect(find.text('10月29日 12:00'), findsNothing);

    await tester.tap(find.byKey(const Key('wechat-restore-start')));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('wechat-restore-error')), findsOneWidget);
    expect(find.text('请先完全退出微信（包括托盘图标），再开始恢复。'), findsOneWidget);
    expect(find.text('恢复到微信'), findsOneWidget);

    harness.bridge.wechatRestoreError = null;
    await tester.tap(find.byKey(const Key('wechat-restore-start')));
    await tester.pumpAndSettle();

    expect(find.text('恢复到微信'), findsNothing);
    expect(harness.bridge.lastWechatRestoreRequest?.restoreDatabase, isTrue);
    expect(harness.bridge.calls, contains('wechatStartRestore:$_accountName'));
    expect(find.text('恢复到微信 · $_accountName'), findsOneWidget);
  });
}
