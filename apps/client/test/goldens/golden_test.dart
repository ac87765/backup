@Tags(<String>['golden'])
library;

import 'dart:async';
import 'dart:io';

import 'package:encrypted_backup_client/app.dart';
import 'package:encrypted_backup_client/common/pages/settings_page.dart';
import 'package:encrypted_backup_client/common/state/file_browser_controller.dart';
import 'package:encrypted_backup_client/common/state/file_drop_controller.dart';
import 'package:encrypted_backup_client/common/state/transfer_controller.dart';
import 'package:encrypted_backup_client/core/rust/models.dart';
import 'package:encrypted_backup_client/desktop/settings/local_settings.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/gestures.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/fake_native_bridge.dart';
import '../support/test_harness.dart';

final bool _fontsExist =
    File(r'C:\Windows\Fonts\SegUIVar.ttf').existsSync() &&
    File(r'C:\Windows\Fonts\msyh.ttc').existsSync();

void main() {
  setUpAll(() async {
    if (!_fontsExist) {
      // ignore: avoid_print
      print(
        'Skipping golden tests: C:\\Windows\\Fonts 缺少 SegUIVar.ttf 或 msyh.ttc',
      );
      return;
    }
    await _loadFont(
      'Segoe UI Variable Text',
      File(r'C:\Windows\Fonts\SegUIVar.ttf'),
    );
    await _loadFont('Microsoft YaHei UI', File(r'C:\Windows\Fonts\msyh.ttc'));
    await _loadFont('Microsoft YaHei', File(r'C:\Windows\Fonts\msyh.ttc'));
    // 本地路径用等宽字体（DesktopMetrics.monoFamily / monoFallback）。
    for (final (family, path) in <(String, String)>[
      ('Cascadia Mono', r'C:\Windows\Fonts\CascadiaMono.ttf'),
      ('Consolas', r'C:\Windows\Fonts\consola.ttf'),
    ]) {
      final file = File(path);
      if (file.existsSync()) {
        await _loadFont(family, file);
      }
    }
    try {
      // 包内字体注册名为 packages/<包名>/<family>，与 IconData.fontPackage 对应。
      final loader = FontLoader('packages/lucide_icons_flutter/Lucide')
        ..addFont(
          rootBundle.load('packages/lucide_icons_flutter/assets/lucide.ttf'),
        );
      await loader.load();
    } catch (error) {
      // ignore: avoid_print
      print('Lucide 字体加载失败，图标可能显示为方框：$error');
    }
  });

  testWidgets(
    'login',
    (tester) async {
      setTestWindow(tester, const Size(880, 560));
      final harness = TestHarness();
      await pumpApp(tester, harness);
      await tester.enterText(
        find.byKey(const Key('login-email')),
        'tlj978626571@gmail.com',
      );
      await tester.enterText(
        find.byKey(const Key('login-password')),
        'correct horse battery',
      );
      await tester.tap(find.byKey(const Key('login-password')));
      await tester.pump();
      await capture(tester, 'login-login-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'login-working',
    (tester) async {
      setTestWindow(tester, const Size(880, 560));
      final harness = TestHarness();
      final completer = Completer<AccountDto>();
      harness.bridge.loginCompleter = completer;
      await pumpApp(tester, harness);
      await tester.enterText(
        find.byKey(const Key('login-email')),
        'tlj978626571@gmail.com',
      );
      await tester.enterText(
        find.byKey(const Key('login-password')),
        'correct horse battery',
      );
      await tester.tap(find.byKey(const Key('login-submit')));
      await tester.pump(const Duration(milliseconds: 50));
      await capture(tester, 'login-working-win.png');
      // 不完成登录：完成后会切到主窗口尺寸并导航到文件页。
      // golden 在“减少动态效果”下运行（setTestWindow 默认开启），
      // 因此按钮底部进度条不绘制、进入动效直接显示最终画面。
      expect(completer.isCompleted, isFalse);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'login-error',
    (tester) async {
      setTestWindow(tester, const Size(880, 560));
      final harness = TestHarness();
      harness.bridge.loginError = const BridgeError(
        code: 'invalid_credentials',
        message: '邮箱或密码不正确',
      );
      await pumpApp(tester, harness);
      await tester.enterText(
        find.byKey(const Key('login-email')),
        'tlj978626571@gmail.com',
      );
      await tester.enterText(
        find.byKey(const Key('login-password')),
        'wrong password',
      );
      await tester.tap(find.byKey(const Key('login-submit')));
      await tester.pumpAndSettle();
      await capture(tester, 'login-error-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'unlock',
    (tester) async {
      setTestWindow(tester, const Size(880, 560));
      final harness = TestHarness(
        bridge: FakeNativeBridge(
          account: testAccount(unlocked: false),
          storedSession: true,
        ),
      );
      await pumpApp(tester, harness);
      await tester.enterText(
        find.byKey(const Key('unlock-password')),
        'correct horse battery',
      );
      await tester.pump();
      await capture(tester, 'unlock-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'pending',
    (tester) async {
      setTestWindow(tester, const Size(880, 560));
      final harness = TestHarness(
        bridge: FakeNativeBridge(
          account: testAccount(status: AccountStatusDto.pending),
          storedSession: true,
        ),
      );
      await pumpApp(tester, harness);
      await capture(tester, 'pending-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'files',
    (tester) async {
      setTestWindow(tester, const Size(1280, 800));
      final harness = _filesHarness();
      await pumpApp(tester, harness);
      await _openFolder(tester, '工作资料');
      await tester.tap(find.byKey(const ValueKey<String>('file-row-q3')));
      await tester.pump(kDoubleTapTimeout);
      final mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
      await mouse.addPointer(location: Offset.zero);
      addTearDown(() => mouse.removePointer());
      await mouse.moveTo(
        tester.getCenter(find.byKey(const ValueKey<String>('file-row-pdf'))),
      );
      await tester.pump();
      await capture(tester, 'files-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'files-menu',
    (tester) async {
      setTestWindow(tester, const Size(1280, 800));
      final harness = _filesHarness();
      await pumpApp(tester, harness);
      await _openFolder(tester, '工作资料');
      await tester.tap(find.byKey(const ValueKey<String>('file-row-pdf')));
      await tester.pump(kDoubleTapTimeout);
      await tester.tap(find.byKey(const ValueKey<String>('file-more-pdf')));
      await tester.pump(kDoubleTapTimeout);
      await tester.pumpAndSettle();
      await capture(tester, 'files-menu-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'files-transfers',
    (tester) async {
      setTestWindow(tester, const Size(1280, 800));
      final harness = TestHarness(
        bridge: FakeNativeBridge(
          account: testAccount(),
          storedSession: true,
          rootItems: [
            testNode(id: 'work', name: '工作资料', kind: NodeKindDto.folder),
          ],
        ),
      );
      harness.bridge.folders['photos'] = <NodeDto>[];
      await pumpApp(tester, harness);
      final container = ProviderScope.containerOf(
        tester.element(find.byType(EncryptedBackupApp)),
        listen: false,
      );
      final transfers = container.read(transferControllerProvider.notifier);

      await transfers.startUploadFiles('root', <String>[
        r'C:\Users\tlj\产品演示-终版.mp4',
      ]);
      await transfers.startUploadFiles('root', <String>[
        r'C:\Users\tlj\IMG_2041.HEIC',
      ]);
      await transfers.startDownloadFolder(
        folderId: 'photos',
        folderName: '照片 / 2024',
        destinationRoot: r'D:\恢复',
      );
      await transfers.startUploadFiles('root', <String>[
        r'C:\Users\tlj\扫描件.pdf',
      ]);

      harness.bridge.emitProgress(
        _progress('transfer-1', TransferStatusDto.uploading, 0, 412000000),
      );
      harness.clock.advance(const Duration(seconds: 30));
      harness.bridge.emitProgress(
        _progress(
          'transfer-1',
          TransferStatusDto.uploading,
          255000000,
          412000000,
        ),
      );
      harness.bridge.emitProgress(
        _progress('transfer-2', TransferStatusDto.retrying, 1100000, 3600000),
      );
      harness.bridge.emitFolderProgress(
        const FolderProgressDto(
          jobId: 'transfer-3',
          status: FolderJobStatusDto.running,
          completedFiles: 942,
          totalFiles: 2140,
          completedBytes: 1000000,
          totalBytes: 2000000,
          counts: FolderCountsDto(
            added: 0,
            updated: 0,
            unchanged: 0,
            failed: 0,
          ),
          current: null,
          error: null,
        ),
      );
      await transfers.pause('transfer-3');
      harness.bridge.emitProgress(
        _progress(
          'transfer-4',
          TransferStatusDto.failed,
          0,
          0,
          error: const TransferFailureDto(
            code: 'file_changed',
            message: '文件在上传过程中被修改',
          ),
        ),
      );
      await tester.pump();
      await tester.tap(find.byKey(const Key('open-transfers')));
      await tester.pumpAndSettle();
      await capture(tester, 'files-transfers-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'settings-general',
    (tester) async {
      setTestWindow(tester, const Size(760, 600));
      final harness = TestHarness(
        bridge: FakeNativeBridge(account: testAccount(), storedSession: true),
      );
      harness.startup.enabled = true;
      await tester.pumpWidget(harness.page(const SettingsPage()));
      await tester.pumpAndSettle();
      await capture(tester, 'settings-general-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'settings-security',
    (tester) async {
      setTestWindow(tester, const Size(760, 600));
      final harness = TestHarness(
        bridge: FakeNativeBridge(account: testAccount(), storedSession: true),
      );
      await tester.pumpWidget(harness.page(const SettingsPage()));
      await tester.pumpAndSettle();
      await tester.tap(find.text('安全'));
      await tester.pumpAndSettle();
      await capture(tester, 'settings-security-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'settings-keep-unlocked-confirm',
    (tester) async {
      setTestWindow(tester, const Size(760, 600));
      final harness = TestHarness(
        bridge: FakeNativeBridge(account: testAccount(), storedSession: true),
      );
      await tester.pumpWidget(harness.page(const SettingsPage()));
      await tester.pumpAndSettle();
      await tester.tap(find.text('安全'));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('keep-unlocked-toggle')));
      await tester.pumpAndSettle();
      await capture(tester, 'settings-keep-unlocked-confirm-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'settings-about',
    (tester) async {
      setTestWindow(tester, const Size(760, 600));
      final harness = TestHarness(
        bridge: FakeNativeBridge(account: testAccount(), storedSession: true),
      );
      await tester.pumpWidget(harness.page(const SettingsPage()));
      await tester.pumpAndSettle();
      await tester.tap(find.text('关于'));
      await tester.pumpAndSettle();
      await capture(tester, 'settings-about-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'backup-folders',
    (tester) async {
      setTestWindow(tester, const Size(1280, 800));
      final harness = _backupHarness();
      await pumpApp(tester, harness);
      await _seedTransfers(tester);
      await tester.tap(find.text('备份文件夹').first);
      await tester.pumpAndSettle();
      await capture(tester, 'backup-folders-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'backup-folders-paused',
    (tester) async {
      setTestWindow(tester, const Size(1280, 800));
      final harness = _backupHarness(paused: true);
      await pumpApp(tester, harness);
      await tester.tap(find.text('备份文件夹').first);
      await tester.pumpAndSettle();
      await capture(tester, 'backup-folders-paused-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'backup-folders-empty',
    (tester) async {
      setTestWindow(tester, const Size(1280, 800));
      final bridge = FakeNativeBridge(
        account: testAccount(),
        storedSession: true,
      );
      bridge.summary = const BackupSummaryDto(
        kind: BackupSummaryStatusKindDto.allBackedUp,
        done: 0,
        total: 0,
        unreadableCount: 0,
        folderCount: 0,
        lastCheckedAt: null,
      );
      final harness = TestHarness(bridge: bridge);
      await pumpApp(tester, harness);
      await tester.tap(find.text('备份文件夹').first);
      await tester.pumpAndSettle();
      await capture(tester, 'backup-folders-empty-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'add-folder',
    (tester) async {
      setTestWindow(tester, const Size(1280, 800));
      final harness = _backupHarness();
      harness.filePicker.directoryToPick = r'D:\工作资料';
      await _openAddFolder(tester, harness);
      await capture(tester, 'add-folder-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'add-folder-quota',
    (tester) async {
      setTestWindow(tester, const Size(1280, 800));
      final harness = _backupHarness();
      harness.bridge.preflight = const BackupPreflightDto(
        fileCount: 18902,
        totalBytes: 52800000000,
        uploadBytes: 52800000000,
        excludedCount: 0,
        unreadableCount: 0,
        remainingQuotaBytes: 37600000000,
        excludedWechatFileCount: 0,
        excludedWechatBytes: 0,
      );
      harness.filePicker.directoryToPick = r'D:\工作资料';
      await _openAddFolder(tester, harness);
      await capture(tester, 'add-folder-quota-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'restore',
    (tester) async {
      setTestWindow(tester, const Size(1280, 800));
      final harness = _filesHarness(includePhotos: true);
      harness.filePicker.directoryToPick = r'D:\恢复';
      await pumpApp(tester, harness);
      await _openFolder(tester, '工作资料');
      await tester.tap(find.byKey(const ValueKey<String>('file-row-photos')));
      await tester.pump(kDoubleTapTimeout);
      await tester.tap(find.byKey(const ValueKey<String>('file-more-photos')));
      await tester.pump(kDoubleTapTimeout);
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const ValueKey<String>('menu-item-下载到电脑…')));
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('restore-pick-destination')));
      await tester.pumpAndSettle();
      await capture(tester, 'restore-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'onboarding-1-folders',
    (tester) async {
      setTestWindow(tester, const Size(760, 720));
      final harness = _onboardingHarness();
      await pumpApp(tester, harness);
      await capture(tester, 'onboarding-1-folders-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'onboarding-2-background',
    (tester) async {
      setTestWindow(tester, const Size(760, 720));
      final harness = _onboardingHarness();
      await pumpApp(tester, harness);
      await tester.tap(find.byKey(const Key('onboarding-next')));
      await tester.pumpAndSettle();
      await capture(tester, 'onboarding-2-background-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'wechat',
    (tester) async {
      setTestWindow(tester, const Size(1280, 800));
      final harness = _wechatHarness();
      await pumpApp(tester, harness);
      await tester.tap(find.text('微信备份').first);
      await tester.pumpAndSettle();
      await capture(tester, 'wechat-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'wechat-off',
    (tester) async {
      setTestWindow(tester, const Size(1280, 800));
      final harness = _wechatHarness(enabled: false);
      await pumpApp(tester, harness);
      await tester.tap(find.text('微信备份').first);
      await tester.pumpAndSettle();
      await capture(tester, 'wechat-off-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'wechat-quota',
    (tester) async {
      setTestWindow(tester, const Size(1280, 800));
      final harness = _wechatHarness(enabled: false, quota: true);
      await pumpApp(tester, harness);
      await tester.tap(find.text('微信备份').first);
      await tester.pumpAndSettle();
      await capture(tester, 'wechat-quota-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'wechat-notfound',
    (tester) async {
      setTestWindow(tester, const Size(1280, 800));
      final harness = TestHarness(
        bridge: FakeNativeBridge(account: testAccount(), storedSession: true),
      );
      await pumpApp(tester, harness);
      await tester.tap(find.text('微信备份').first);
      await tester.pumpAndSettle();
      await capture(tester, 'wechat-notfound-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'wechat-restore',
    (tester) async {
      setTestWindow(tester, const Size(1280, 800));
      final harness = _wechatHarness();
      await pumpApp(tester, harness);
      await tester.tap(find.text('微信备份').first);
      await tester.pumpAndSettle();
      await tester.tap(find.byKey(const Key('wechat-restore')));
      await tester.pumpAndSettle();
      await capture(tester, 'wechat-restore-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'files-drop',
    (tester) async {
      setTestWindow(tester, const Size(1280, 800));
      final harness = _filesHarness();
      await pumpApp(tester, harness);
      await _openFolder(tester, '工作资料');
      final container = ProviderScope.containerOf(
        tester.element(find.byType(EncryptedBackupApp)),
        listen: false,
      );
      container.read(fileDropOverlayProvider.notifier).setVisible(true);
      await tester.pump();
      await capture(tester, 'files-drop-win.png');
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );
}

WechatCategoryInfoDto _wechatInfo(
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

WechatAccountDto _wechatAccount({
  required String name,
  required int totalBytes,
  required List<WechatCategoryInfoDto> categories,
  bool current = true,
}) {
  return WechatAccountDto(
    accountName: name,
    version: WechatVersionDto.wechat4,
    dataRoot: r'C:\Users\tlj\Documents\xwechat_files',
    accountDir: 'C:\\Users\\tlj\\Documents\\xwechat_files\\$name',
    source: WechatSourceDto.defaultLocation,
    categories: categories,
    totalSizeBytes: totalBytes,
    totalFileCount: 1000,
    lastModifiedNs: 1761900000000000000,
    isCurrentLoginCandidate: current,
    hasPhoneMigrationBackup: false,
    note: null,
  );
}

WechatAccountBackupStatusDto _wechatStatus(
  WechatAccountDto account, {
  required Set<WechatCategoryDto> selected,
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

TestHarness _wechatHarness({bool enabled = true, bool quota = false}) {
  const primaryName = 'wxid_k8f3abc27';
  const secondaryName = 'wxid_q2m9def61';
  final primary = _wechatAccount(
    name: primaryName,
    totalBytes: 3100000000,
    categories: <WechatCategoryInfoDto>[
      _wechatInfo(WechatCategoryDto.chatFiles, bytes: 205000000),
      _wechatInfo(WechatCategoryDto.imagesAndVideos, bytes: 2400000000),
      _wechatInfo(WechatCategoryDto.databases, bytes: 163000000),
      _wechatInfo(WechatCategoryDto.phoneMigration, exists: false),
    ],
  );
  final secondary = _wechatAccount(
    name: secondaryName,
    totalBytes: 88000000,
    current: false,
    categories: <WechatCategoryInfoDto>[
      _wechatInfo(WechatCategoryDto.chatFiles, bytes: 400000),
      _wechatInfo(WechatCategoryDto.imagesAndVideos, exists: false),
      _wechatInfo(WechatCategoryDto.databases, exists: false),
      _wechatInfo(WechatCategoryDto.phoneMigration, exists: false),
    ],
  );
  final bridge = FakeNativeBridge(account: testAccount(), storedSession: true);
  bridge.wechatAccounts = <WechatAccountDto>[primary, secondary];
  bridge.wechatPreflightBuilder = (account, categories) {
    final withImages = categories.contains(WechatCategoryDto.imagesAndVideos);
    if (account.accountName == secondaryName) {
      return const WechatBackupPreflightDto(
        fileCount: 0,
        totalBytes: 0,
        uploadBytes: 0,
        uploadBytesWithoutImagesAndVideos: 0,
        databaseBytes: 0,
        excludedCount: 0,
        unreadableCount: 0,
        remainingQuotaBytes: 37600000000,
        overQuota: false,
      );
    }
    return WechatBackupPreflightDto(
      fileCount: withImages ? 1284 : 420,
      totalBytes: withImages ? 2800000000 : 400000000,
      uploadBytes: withImages ? 2800000000 : 400000000,
      uploadBytesWithoutImagesAndVideos: 400000000,
      databaseBytes: 163000000,
      excludedCount: 0,
      unreadableCount: 0,
      remainingQuotaBytes: quota ? 1900000000 : 37600000000,
      overQuota: quota && withImages,
    );
  };
  if (enabled) {
    bridge.wechatStatuses = <WechatAccountBackupStatusDto>[
      _wechatStatus(
        primary,
        selected: <WechatCategoryDto>{
          WechatCategoryDto.chatFiles,
          WechatCategoryDto.imagesAndVideos,
          WechatCategoryDto.databases,
        },
        statuses: <WechatCategoryDto, BackupFolderStatusDto>{
          WechatCategoryDto.chatFiles: const BackupFolderStatusDto(
            kind: BackupFolderStatusKindDto.idleOk,
            done: 0,
            total: 0,
            unreadableCount: 0,
          ),
          WechatCategoryDto.imagesAndVideos: const BackupFolderStatusDto(
            kind: BackupFolderStatusKindDto.uploading,
            done: 128,
            total: 940,
            unreadableCount: 0,
          ),
          WechatCategoryDto.databases: const BackupFolderStatusDto(
            kind: BackupFolderStatusKindDto.waitingForWechatExit,
            done: 0,
            total: 0,
            unreadableCount: 0,
          ),
        },
        lastBackups: <WechatCategoryDto, int>{
          WechatCategoryDto.chatFiles: _at(10, 0),
          WechatCategoryDto.imagesAndVideos: _at(10, 0),
          WechatCategoryDto.databases:
              DateTime(2026, 10, 30, 23, 10).millisecondsSinceEpoch ~/ 1000,
        },
      ),
      _wechatStatus(
        secondary,
        selected: <WechatCategoryDto>{WechatCategoryDto.chatFiles},
        statuses: <WechatCategoryDto, BackupFolderStatusDto>{
          WechatCategoryDto.chatFiles: const BackupFolderStatusDto(
            kind: BackupFolderStatusKindDto.idleOk,
            done: 0,
            total: 0,
            unreadableCount: 0,
          ),
        },
        lastBackups: <WechatCategoryDto, int>{
          WechatCategoryDto.chatFiles: _at(8, 30),
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
        timestamp: '20261028T224500Z',
        snapshotAt:
            DateTime(2026, 10, 28, 22, 45).millisecondsSinceEpoch ~/ 1000,
        totalBytes: 161000000,
        fileCount: 31,
        complete: true,
      ),
      WechatDatabaseSnapshotDto(
        deviceName: 'DESKTOP-TLJ',
        timestamp: '20261024T190200Z',
        snapshotAt:
            DateTime(2026, 10, 24, 19, 2).millisecondsSinceEpoch ~/ 1000,
        totalBytes: 158000000,
        fileCount: 30,
        complete: true,
      ),
    ];
  }
  return TestHarness(bridge: bridge);
}

Future<void> _loadFont(String family, File file) async {
  final loader = FontLoader(family)
    ..addFont(file.readAsBytes().then((bytes) => ByteData.sublistView(bytes)));
  await loader.load();
}

/// 截图时打开阴影（测试框架默认关闭），截完立即恢复：debug 变量必须在测试结束前复原。
Future<void> capture(WidgetTester tester, String name) async {
  debugDisableShadows = false;
  try {
    for (final view in tester.binding.renderViews) {
      view.markNeedsPaint();
    }
    await tester.pump();
    await expectLater(
      find.byKey(const Key('golden-root')),
      matchesGoldenFile(name),
    );
  } finally {
    debugDisableShadows = true;
  }
}

TestHarness _filesHarness({bool includePhotos = false}) {
  final bridge = FakeNativeBridge(
    account: testAccount(),
    storedSession: true,
    rootItems: [testNode(id: 'work', name: '工作资料', kind: NodeKindDto.folder)],
  );
  int at(int month, int day, int hour, int minute) =>
      DateTime(2026, month, day, hour, minute).millisecondsSinceEpoch ~/ 1000;
  bridge.folders['work'] = <NodeDto>[
    testNode(
      id: 'contracts',
      name: '合同',
      kind: NodeKindDto.folder,
      updatedAt: DateTime(2026, 10, 31, 9, 12).toIso8601String(),
    ),
    testNode(
      id: 'designs',
      name: '设计稿',
      kind: NodeKindDto.folder,
      updatedAt: DateTime(2026, 10, 30, 18, 40).toIso8601String(),
    ),
    if (includePhotos)
      testNode(
        id: 'photos',
        name: '照片 / 2024',
        kind: NodeKindDto.folder,
        updatedAt: DateTime(2026, 10, 30, 18, 40).toIso8601String(),
      ),
    testNode(
      id: 'pdf',
      name: '2026 年度合作协议.pdf',
      size: 2400000,
      mtime: at(10, 31, 10, 5),
      currentVersionId: 'version-pdf',
    ),
    testNode(
      id: 'q3',
      name: 'Q3 预算.xlsx',
      size: 186000,
      mtime: at(10, 27, 16, 22),
      currentVersionId: 'version-q3',
    ),
    testNode(
      id: 'weekly',
      name: '项目周报-第43周.docx',
      size: 48000,
      mtime: at(10, 25, 11, 3),
      currentVersionId: 'version-weekly',
    ),
    testNode(
      id: 'whiteboard',
      name: '白板照片.jpg',
      size: 3100000,
      mtime: at(10, 24, 15, 47),
      currentVersionId: 'version-photo',
    ),
    testNode(
      id: 'demo',
      name: '产品演示.mp4',
      size: 412000000,
      mtime: at(10, 20, 20, 15),
      currentVersionId: 'version-demo',
    ),
    testNode(
      id: 'archive',
      name: '旧版本素材.zip',
      size: 1200000000,
      mtime: at(9, 30, 9, 0),
      currentVersionId: 'version-archive',
    ),
  ];
  return TestHarness(bridge: bridge);
}

int _at(int hour, int minute) =>
    DateTime(2026, 10, 31, hour, minute).millisecondsSinceEpoch ~/ 1000;

BackupFolderDto _goldenBackupFolder({
  required String id,
  required String name,
  required String localPath,
  required int? lastBackupAt,
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
    enabled: true,
    lastBackupAt: lastBackupAt,
  );
}

BackupFolderStateDto _goldenBackupState(
  String id, {
  BackupFolderStatusKindDto kind = BackupFolderStatusKindDto.idleOk,
  int done = 0,
  int total = 0,
  int unreadable = 0,
  int? lastBackupAt,
}) {
  return BackupFolderStateDto(
    folderId: id,
    status: BackupFolderStatusDto(
      kind: kind,
      done: done,
      total: total,
      unreadableCount: unreadable,
    ),
    pauseReason: kind == BackupFolderStatusKindDto.paused
        ? BackupPauseReasonDto.user
        : null,
    lastBackupAt: lastBackupAt,
    wechat: null,
  );
}

TestHarness _backupHarness({bool paused = false}) {
  final bridge = FakeNativeBridge(account: testAccount(), storedSession: true);
  bridge.backupFolders = <BackupFolderDto>[
    _goldenBackupFolder(
      id: 'work',
      name: '工作资料',
      localPath: r'D:\工作资料',
      lastBackupAt: _at(10, 0),
    ),
    _goldenBackupFolder(
      id: 'photos',
      name: '照片',
      localPath: r'C:\Users\tlj\Pictures',
      lastBackupAt: _at(9, 59),
    ),
    _goldenBackupFolder(
      id: 'docs',
      name: '文档',
      localPath: r'C:\Users\tlj\Documents',
      lastBackupAt: _at(8, 30),
    ),
  ];
  bridge.backupStates = <BackupFolderStateDto>[
    _goldenBackupState('work', lastBackupAt: _at(10, 0)),
    _goldenBackupState(
      'photos',
      kind: BackupFolderStatusKindDto.uploading,
      done: 128,
      total: 340,
      lastBackupAt: _at(9, 59),
    ),
    _goldenBackupState(
      'docs',
      kind: paused
          ? BackupFolderStatusKindDto.paused
          : BackupFolderStatusKindDto.error,
      unreadable: paused ? 0 : 2,
      lastBackupAt: _at(8, 30),
    ),
  ];
  bridge.backupActivities = <BackupActivityDto>[
    BackupActivityDto(
      id: 1,
      folderId: 'photos',
      relativePath: 'IMG_2041.HEIC',
      kind: BackupActivityKindDto.added,
      occurredAt: _at(10, 5),
      wechat: null,
    ),
    BackupActivityDto(
      id: 2,
      folderId: 'work',
      relativePath: 'Q3 预算.xlsx',
      kind: BackupActivityKindDto.newVersion,
      occurredAt: _at(10, 0),
      wechat: null,
    ),
    BackupActivityDto(
      id: 3,
      folderId: 'docs',
      relativePath: r'~$周报.docx',
      kind: BackupActivityKindDto.retryLater,
      occurredAt: _at(10, 4),
      wechat: null,
    ),
    BackupActivityDto(
      id: 4,
      folderId: 'work',
      relativePath: '2026 年度合作协议.pdf',
      kind: BackupActivityKindDto.unchanged,
      occurredAt: _at(10, 5),
      wechat: null,
    ),
  ];
  bridge.summary = BackupSummaryDto(
    kind: paused
        ? BackupSummaryStatusKindDto.paused
        : BackupSummaryStatusKindDto.uploading,
    done: 328,
    total: 340,
    unreadableCount: 2,
    folderCount: 3,
    lastCheckedAt: _at(10, 5),
  );
  return TestHarness(bridge: bridge);
}

/// 让状态栏显示“传输 · 3 项进行中”（与设计稿一致）。
Future<void> _seedTransfers(WidgetTester tester) async {
  final container = ProviderScope.containerOf(
    tester.element(find.byType(EncryptedBackupApp)),
    listen: false,
  );
  final transfers = container.read(transferControllerProvider.notifier);
  await transfers.startUploadFiles('root', <String>[
    r'C:\Users\tlj\产品演示-终版.mp4',
  ]);
  await transfers.startUploadFiles('root', <String>[
    r'C:\Users\tlj\IMG_2041.HEIC',
  ]);
  await transfers.startUploadFolder('root', r'C:\Users\tlj\照片');
  await tester.pump();
}

Future<void> _openAddFolder(WidgetTester tester, TestHarness harness) async {
  await pumpApp(tester, harness);
  await _seedTransfers(tester);
  await tester.tap(find.text('备份文件夹').first);
  await tester.pumpAndSettle();
  await tester.tap(find.byKey(const Key('backup-add-folder')));
  await tester.pumpAndSettle();
  await tester.tap(find.byKey(const Key('backup-pick-folder')));
  await tester.pumpAndSettle();
  for (final rule in <String>['node_modules', '*.psd']) {
    await tester.tap(find.byKey(const Key('exclude-rule-add')));
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const Key('exclude-rule-input')), rule);
    await tester.tap(find.byKey(const Key('exclude-rule-confirm')));
    await tester.pumpAndSettle();
  }
  await tester.tap(find.byKey(const ValueKey<String>('subfolder-toggle-临时导出')));
  await tester.pumpAndSettle();
}

TestHarness _onboardingHarness() {
  final harness = TestHarness(
    bridge: FakeNativeBridge(account: testAccount(), storedSession: true),
    localSettings: const LocalSettings(),
  );
  harness.bridge.preflightByPath.addAll(<String, BackupPreflightDto>{
    r'C:\Users\tlj\Desktop': const BackupPreflightDto(
      fileCount: 326,
      totalBytes: 1400000000,
      uploadBytes: 1400000000,
      excludedCount: 0,
      unreadableCount: 0,
      remainingQuotaBytes: 37600000000,
      excludedWechatFileCount: 0,
      excludedWechatBytes: 0,
    ),
    r'C:\Users\tlj\Documents': const BackupPreflightDto(
      fileCount: 2018,
      totalBytes: 4900000000,
      uploadBytes: 4900000000,
      excludedCount: 0,
      unreadableCount: 0,
      remainingQuotaBytes: 37600000000,
      excludedWechatFileCount: 0,
      excludedWechatBytes: 0,
    ),
    r'C:\Users\tlj\Pictures': const BackupPreflightDto(
      fileCount: 8410,
      totalBytes: 31200000000,
      uploadBytes: 31200000000,
      excludedCount: 0,
      unreadableCount: 0,
      remainingQuotaBytes: 37600000000,
      excludedWechatFileCount: 0,
      excludedWechatBytes: 0,
    ),
  });
  return harness;
}

Future<void> _openFolder(WidgetTester tester, String name) async {
  final container = ProviderScope.containerOf(
    tester.element(find.byType(EncryptedBackupApp)),
    listen: false,
  );
  final state = container.read(fileBrowserProvider);
  final folder = state.items.firstWhere(
    (item) => item.name == name && item.kind == NodeKindDto.folder,
  );
  await container.read(fileBrowserProvider.notifier).openFolder(folder);
  await tester.pumpAndSettle();
}

TransferProgressDto _progress(
  String id,
  TransferStatusDto status,
  int completed,
  int total, {
  TransferFailureDto? error,
}) {
  return TransferProgressDto(
    transferId: id,
    direction: TransferDirectionDto.upload,
    status: status,
    completedBytes: completed,
    totalBytes: total,
    completedChunks: 0,
    totalChunks: 0,
    error: error,
  );
}
