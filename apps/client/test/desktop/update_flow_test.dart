import 'package:encrypted_backup_client/app.dart';
import 'package:encrypted_backup_client/common/pages/settings_page.dart';
import 'package:encrypted_backup_client/common/state/auth_controller.dart';
import 'package:encrypted_backup_client/core/platform/window_controls.dart';
import 'package:encrypted_backup_client/core/rust/models.dart';
import 'package:encrypted_backup_client/desktop/state/update_controller.dart';
import 'package:encrypted_backup_client/desktop/tray/tray_service.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/fake_native_bridge.dart';
import '../support/test_harness.dart';

const _notes = <String>['新增微信备份：聊天文件、图片视频与聊天记录快照，可恢复到微信', '托盘菜单与退出修复'];

UpdateInfoDto _info({
  String version = '0.2.0',
  bool forced = false,
  List<String> notes = _notes,
}) {
  return UpdateInfoDto(
    version: version,
    releasedAt: '2026-11-05T10:00:00Z',
    notes: notes,
    size: 15400000,
    forced: forced,
  );
}

UpdateCheckDto _available({bool forced = false, List<String> notes = _notes}) {
  return UpdateCheckDto(
    state: UpdateCheckStateDto.available,
    info: _info(forced: forced, notes: notes),
    checkedAt: 1761905100,
  );
}

const _failedMessage = '下载的文件不完整或被篡改，已删除，没有运行。可能是网络中断或被安全软件修改。';

TestHarness _harness({FakeNativeBridge? bridge}) {
  return TestHarness(
    bridge:
        bridge ?? FakeNativeBridge(account: testAccount(), storedSession: true),
  );
}

UpdateController _controller(WidgetTester tester) {
  return ProviderScope.containerOf(
    tester.element(find.byType(EncryptedBackupApp)),
    listen: false,
  ).read(updateControllerProvider.notifier);
}

Future<void> _check(WidgetTester tester, TestHarness harness) async {
  await _controller(tester).checkNow();
  await tester.pump();
}

void _emitProgress(TestHarness harness, UpdateProgressDto progress) {
  harness.bridge.emitUpdateProgress(progress);
}

UpdateProgressDto _progress(
  UpdateDownloadStatusDto status, {
  int downloaded = 0,
  int total = 15400000,
  int speed = 0,
  TransferFailureDto? error,
}) {
  return UpdateProgressDto(
    downloadId: 'update-1',
    status: status,
    downloadedBytes: downloaded,
    totalBytes: total,
    speedBytesPerSecond: speed,
    error: error,
  );
}

void main() {
  testWidgets('有新版本时显示横幅，关闭后同版本不再出现', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = _harness();
    await pumpApp(tester, harness);
    harness.bridge.updateCheckResult = _available();
    await _check(tester, harness);

    expect(find.byKey(const Key('update-banner')), findsOneWidget);
    expect(find.text('新版本 0.2.0 可用'), findsOneWidget);

    await tester.tap(find.byKey(const Key('update-banner-close')));
    await tester.pump();
    expect(find.byKey(const Key('update-banner')), findsNothing);
    expect(harness.bridge.calls, contains('updateDismissBanner:0.2.0'));

    await _check(tester, harness);
    expect(find.byKey(const Key('update-banner')), findsNothing);
  });

  testWidgets('横幅“更新内容”打开设置关于，“下载并安装”开始下载并切到关于', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = _harness();
    await pumpApp(tester, harness);
    harness.bridge.updateCheckResult = _available();
    await _check(tester, harness);

    await tester.tap(find.byKey(const Key('update-banner-notes')));
    await tester.pumpAndSettle();
    expect(find.text('当前版本'), findsOneWidget);
    expect(find.text('有新版本'), findsOneWidget);

    final container = ProviderScope.containerOf(
      tester.element(find.byType(EncryptedBackupApp)),
      listen: false,
    );
    container.read(routerProvider).go('/files');
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('update-banner-download')));
    await tester.pumpAndSettle();
    expect(harness.bridge.calls, contains('updateDownload'));
    expect(find.byKey(const Key('update-card-downloading')), findsOneWidget);
  });

  testWidgets('开发构建不显示横幅，关于显示开发版本', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = _harness();
    await pumpApp(tester, harness);
    harness.bridge.updateCheckResult = const UpdateCheckDto(
      state: UpdateCheckStateDto.developmentBuild,
      checkedAt: 1761905100,
    );
    await _check(tester, harness);
    expect(find.byKey(const Key('update-banner')), findsNothing);

    final router = ProviderScope.containerOf(
      tester.element(find.byType(EncryptedBackupApp)),
      listen: false,
    ).read(routerProvider);
    router.go('/settings?tab=about');
    await tester.pumpAndSettle();
    expect(find.text('开发版本，不检查更新'), findsOneWidget);
  });

  testWidgets('关于页在下载、校验失败、已是最新与无法检查之间切换', (tester) async {
    setTestWindow(tester, const Size(760, 600));
    final harness = _harness();
    harness.bridge.updateCheckResult = _available();
    await tester.pumpWidget(
      harness.page(const SettingsPage(initialTab: SettingsTab.about)),
    );
    await tester.pumpAndSettle();

    final container = ProviderScope.containerOf(
      tester.element(find.byType(SettingsPage)),
      listen: false,
    );
    await container.read(updateControllerProvider.notifier).checkNow();
    await tester.pump();
    expect(find.text('有新版本'), findsOneWidget);
    expect(find.text('已验证发布签名'), findsOneWidget);
    expect(
      harness.bridge.calls.where((call) => call == 'updateCancelDownload'),
      isEmpty,
    );

    _emitProgress(
      harness,
      _progress(
        UpdateDownloadStatusDto.downloading,
        downloaded: 9600000,
        speed: 2100000,
      ),
    );
    await tester.pump();
    await tester.pump();
    expect(find.text('正在下载'), findsOneWidget);
    expect(find.text('9.6 MB / 15.4 MB · 2.1 MB/s'), findsOneWidget);

    await tester.tap(find.byKey(const Key('update-cancel')));
    await tester.pump();
    expect(harness.bridge.calls, contains('updateCancelDownload'));
    expect(find.text('有新版本'), findsOneWidget);

    _emitProgress(
      harness,
      _progress(
        UpdateDownloadStatusDto.failed,
        error: const TransferFailureDto(
          code: 'integrity_error',
          message: _failedMessage,
        ),
      ),
    );
    await tester.pump();
    await tester.pump();
    expect(find.text('无法安装 0.2.0'), findsOneWidget);
    expect(find.text(_failedMessage), findsOneWidget);

    harness.bridge.updateCheckResult = const UpdateCheckDto(
      state: UpdateCheckStateDto.upToDate,
      checkedAt: 1761905200,
    );
    await container.read(updateControllerProvider.notifier).checkNow();
    await tester.pump();
    expect(find.text('已是最新版本'), findsOneWidget);

    harness.bridge.updateCheckResult = const UpdateCheckDto(
      state: UpdateCheckStateDto.unavailable,
      checkedAt: 1761905300,
      reason: 'network',
    );
    await container.read(updateControllerProvider.notifier).checkNow();
    await tester.pump();
    expect(find.text('上次检查：无法获取更新信息'), findsOneWidget);
    expect(find.byKey(const Key('update-check')), findsOneWidget);
  });

  testWidgets('下载完成弹出重新启动对话框，立即重新启动调用 install 与退出流程', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = _harness();
    await pumpApp(tester, harness);
    harness.bridge.updateCheckResult = _available();
    await _check(tester, harness);
    final router = ProviderScope.containerOf(
      tester.element(find.byType(EncryptedBackupApp)),
      listen: false,
    ).read(routerProvider);
    router.go('/settings?tab=about');
    await tester.pumpAndSettle();

    _emitProgress(
      harness,
      _progress(UpdateDownloadStatusDto.completed, downloaded: 15400000),
    );
    await tester.pump();
    await tester.pumpAndSettle();
    expect(find.text('重新启动以完成更新？'), findsOneWidget);

    await tester.tap(find.byKey(const Key('update-restart-now')));
    await tester.pumpAndSettle();
    expect(harness.bridge.calls, contains('updateInstall'));
    expect(harness.appQuit.quitCalls, 1);
  });

  testWidgets('manual_install_required 打开 dmg 并提示拖到应用程序', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = _harness();
    await pumpApp(tester, harness);
    harness.bridge.updateCheckResult = _available();
    await _check(tester, harness);
    harness.bridge.updateInstallError = const BridgeError(
      code: 'manual_install_required',
      message: '无法替换当前应用，安装包已打开，请把“加密备份”拖到“应用程序”',
    );
    final router = ProviderScope.containerOf(
      tester.element(find.byType(EncryptedBackupApp)),
      listen: false,
    ).read(routerProvider);
    router.go('/settings?tab=about');
    await tester.pumpAndSettle();

    _emitProgress(
      harness,
      _progress(UpdateDownloadStatusDto.completed, downloaded: 15400000),
    );
    await tester.pump();
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('update-restart-now')));
    await tester.pumpAndSettle();

    expect(harness.bridge.calls, contains('updateOpenDmg'));
    expect(harness.appQuit.quitCalls, 0);
    expect(find.text('请把加密备份拖到“应用程序”'), findsOneWidget);
  });

  testWidgets('强制更新页替换主界面且不能通过导航离开，不启动备份', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = _harness();
    await pumpApp(tester, harness);
    harness.bridge.updateCheckResult = _available(forced: true);
    await _check(tester, harness);
    await tester.pumpAndSettle();

    expect(find.text('需要更新后才能继续使用'), findsOneWidget);
    expect(find.text('下载并安装'), findsOneWidget);
    expect(find.text('打开下载页面'), findsOneWidget);
    expect(harness.windowControls.modes, contains(WindowMode.forced));

    final container = ProviderScope.containerOf(
      tester.element(find.byType(EncryptedBackupApp)),
      listen: false,
    );
    container.read(routerProvider).go('/files');
    await tester.pumpAndSettle();
    expect(find.text('需要更新后才能继续使用'), findsOneWidget);

    await tester.tap(find.byKey(const Key('forced-download-page')));
    await tester.pumpAndSettle();
    expect(
      harness.openedUris,
      contains(Uri.parse('https://120.79.245.99/download')),
    );

    expect(
      harness.bridge.calls.where((call) => call.startsWith('backupStart')),
      isEmpty,
    );
    expect(
      harness.bridge.calls.where(
        (call) =>
            call.startsWith('logout') || call.startsWith('forgetCredentials'),
      ),
      isEmpty,
    );
  });

  testWidgets('未登录时发现强制更新也替换登录页', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = _harness(bridge: FakeNativeBridge());
    await pumpApp(tester, harness);
    expect(find.byKey(const Key('login-title')), findsOneWidget);

    harness.bridge.updateCheckResult = _available(forced: true);
    await _check(tester, harness);
    await tester.pumpAndSettle();

    expect(find.text('需要更新后才能继续使用'), findsOneWidget);
    expect(find.byKey(const Key('login-title')), findsNothing);
  });

  testWidgets('无法获取更新信息时不提示横幅', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = _harness();
    await pumpApp(tester, harness);
    harness.bridge.updateCheckResult = const UpdateCheckDto(
      state: UpdateCheckStateDto.unavailable,
      checkedAt: 1761905100,
      reason: 'network',
    );
    await _check(tester, harness);
    expect(find.byKey(const Key('update-banner')), findsNothing);
  });

  testWidgets('强制更新时托盘不启动备份，解除后才启动', (tester) async {
    final harness = _harness();
    final container = ProviderContainer(overrides: harness.overrides);
    addTearDown(container.dispose);
    final tray = container.read(trayServiceProvider);

    tray.updateUpdate(UpdateState(check: _available(forced: true)));
    tray.updateAuth(AuthStage.ready);
    await tester.pump();
    expect(
      harness.bridge.calls.where((call) => call == 'backupStart'),
      isEmpty,
    );
    expect(harness.bridge.calls, contains('backupStop'));

    tray.updateUpdate(const UpdateState());
    tray.updateAuth(AuthStage.ready);
    await tester.pump();
    expect(harness.bridge.calls, contains('backupStart'));
  });

  testWidgets('启动 10 秒后首次检查，之后每 24 小时检查一次', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = _harness();
    await pumpApp(tester, harness);
    int checkCount() => harness.bridge.calls
        .where((call) => call.startsWith('updateCheck:'))
        .length;

    expect(checkCount(), 0);
    expect(harness.updateScheduler.nextDelay, const Duration(seconds: 10));

    harness.updateScheduler.fire();
    await tester.pump();
    await tester.pump();
    expect(checkCount(), 1);
    final expectedNow = harness.clock.current.millisecondsSinceEpoch ~/ 1000;
    expect(
      harness.bridge.calls.where((call) => call.startsWith('updateCheck:')),
      contains('updateCheck:$expectedNow'),
    );
    expect(harness.updateScheduler.nextDelay, const Duration(hours: 24));

    harness.updateScheduler.fire();
    await tester.pump();
    await tester.pump();
    expect(checkCount(), 2);
  });
}
