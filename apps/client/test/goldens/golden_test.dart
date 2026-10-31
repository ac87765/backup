@Tags(<String>['golden'])
library;

import 'dart:async';
import 'dart:io';

import 'package:encrypted_backup_client/app.dart';
import 'package:encrypted_backup_client/common/pages/settings_page.dart';
import 'package:encrypted_backup_client/common/state/file_browser_controller.dart';
import 'package:encrypted_backup_client/common/state/transfer_controller.dart';
import 'package:encrypted_backup_client/core/rust/models.dart';
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
      setTestWindow(tester, const Size(440, 600));
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
      setTestWindow(tester, const Size(440, 600));
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
      // 不完成登录：完成后会在 440×600 的登录窗口尺寸里导航到文件页。
      expect(completer.isCompleted, isFalse);
    },
    variant: TargetPlatformVariant.only(TargetPlatform.windows),
    skip: !_fontsExist,
  );

  testWidgets(
    'login-error',
    (tester) async {
      setTestWindow(tester, const Size(440, 600));
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
      setTestWindow(tester, const Size(440, 600));
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
      setTestWindow(tester, const Size(440, 600));
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

TestHarness _filesHarness() {
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
