import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_native_bridge.dart';
import 'support/test_harness.dart';

void main() {
  testWidgets('打开设置并关闭保持解锁的确认对话框不会调用桥接', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(account: testAccount(), storedSession: true),
    );
    await pumpApp(tester, harness);

    await tester.tap(find.byKey(const Key('open-settings')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('安全'));
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const Key('keep-unlocked-toggle')));
    await tester.pumpAndSettle();
    expect(find.text('开启“保持解锁”？'), findsOneWidget);
    await tester.tap(find.byKey(const Key('keep-unlocked-cancel')));
    await tester.pumpAndSettle();

    expect(find.text('开启“保持解锁”？'), findsNothing);
    expect(
      harness.bridge.calls.where((call) => call.startsWith('setKeepUnlocked:')),
      isEmpty,
    );
  });

  testWidgets('输入密码确认后开启保持解锁', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(account: testAccount(), storedSession: true),
    );
    await pumpApp(tester, harness);

    await tester.tap(find.byKey(const Key('open-settings')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('安全'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('keep-unlocked-toggle')));
    await tester.pumpAndSettle();

    await tester.enterText(
      find.byKey(const Key('keep-unlocked-password')),
      'secret',
    );
    await tester.tap(find.byKey(const Key('keep-unlocked-confirm')));
    await tester.pumpAndSettle();

    expect(harness.bridge.calls, contains('unlock'));
    expect(harness.bridge.calls, contains('setKeepUnlocked:true'));
    expect(harness.bridge.keepUnlocked, isTrue);
  });

  testWidgets('通用设置接上开机启动、上传限速与隐藏文件', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(account: testAccount(), storedSession: true),
    );
    await pumpApp(tester, harness);
    await tester.tap(find.byKey(const Key('open-settings')));
    await tester.pumpAndSettle();

    expect(find.text('使用电池时暂停备份'), findsNothing);
    expect(find.textContaining('将在后续版本接入'), findsNothing);

    await tester.tap(find.byKey(const Key('startup-toggle')));
    await tester.pumpAndSettle();
    expect(harness.startup.calls, <bool>[true]);

    await tester.tap(find.byKey(const Key('speed-limit-select')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey<String>('menu-item-5 MB/s')));
    await tester.pumpAndSettle();
    expect(
      harness.bridge.calls,
      contains('backupSetSettings:includeHidden=false:limit=5000000'),
    );

    await tester.tap(find.byKey(const Key('backup-hidden-toggle')));
    await tester.pumpAndSettle();
    expect(
      harness.bridge.calls,
      contains('backupSetSettings:includeHidden=true:limit=5000000'),
    );
  });

  testWidgets('聊天记录快照保留份数写入桥接设置', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(account: testAccount(), storedSession: true),
    );
    await pumpApp(tester, harness);
    await tester.tap(find.byKey(const Key('open-settings')));
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const Key('wechat-retention-select')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey<String>('menu-item-5 份')));
    await tester.pumpAndSettle();

    expect(harness.bridge.backupSettingsValue.wechatSnapshotRetention, 5);
    expect(find.text('5 份'), findsOneWidget);
  });

  testWidgets('设置页退出登录回到登录页', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(account: testAccount(), storedSession: true),
    );
    await pumpApp(tester, harness);

    await tester.tap(find.byKey(const Key('open-settings')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('安全'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('settings-logout')));
    await tester.pumpAndSettle();

    expect(find.byKey(const Key('login-title')), findsOneWidget);
    expect(harness.bridge.calls, contains('logout'));
  });
}
