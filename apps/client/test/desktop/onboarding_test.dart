import 'package:encrypted_backup_client/desktop/settings/local_settings.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/fake_native_bridge.dart';
import '../support/test_harness.dart';

void main() {
  testWidgets('首次登录显示引导，完成标记与选中文件夹写入', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(account: testAccount(), storedSession: true),
      localSettings: const LocalSettings(),
    );
    await pumpApp(tester, harness);

    expect(find.text('选择要自动备份的文件夹'), findsOneWidget);
    expect(find.text('桌面'), findsOneWidget);
    expect(find.text('文档'), findsOneWidget);
    expect(find.text('图片'), findsOneWidget);

    await tester.tap(find.byKey(const Key('onboarding-next')));
    await tester.pumpAndSettle();
    expect(find.text('在后台继续备份'), findsOneWidget);

    await tester.tap(find.byKey(const Key('onboarding-keep-unlocked-toggle')));
    await tester.pumpAndSettle();
    expect(find.text('开启“保持解锁”？'), findsOneWidget);
    await tester.tap(find.byKey(const Key('keep-unlocked-cancel')));
    await tester.pumpAndSettle();
    expect(harness.bridge.calls, isNot(contains('setKeepUnlocked:true')));

    await tester.tap(find.byKey(const Key('onboarding-startup-toggle')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('onboarding-start')));
    await tester.pumpAndSettle();

    expect(
      harness.bridge.calls,
      contains(r'backupAddFolder:C:\Users\tlj\Desktop'),
    );
    expect(
      harness.bridge.calls,
      contains(r'backupAddFolder:C:\Users\tlj\Documents'),
    );
    expect(
      harness.bridge.calls,
      isNot(contains(r'backupAddFolder:C:\Users\tlj\Pictures')),
    );
    expect(harness.startup.calls, <bool>[true]);
    expect(harness.settingsStore.settings.onboardingCompleted, isTrue);
    expect(find.text('全部文件'), findsWidgets);
  });

  testWidgets('稍后再说跳过引导并保存完成标记', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(account: testAccount(), storedSession: true),
      localSettings: const LocalSettings(),
    );
    await pumpApp(tester, harness);
    expect(find.text('选择要自动备份的文件夹'), findsOneWidget);

    await tester.tap(find.byKey(const Key('onboarding-skip')));
    await tester.pumpAndSettle();

    expect(harness.settingsStore.settings.onboardingCompleted, isTrue);
    expect(harness.bridge.calls, isNot(contains('backupAddFolder')));
    expect(find.text('全部文件'), findsWidgets);
  });

  testWidgets('已完成引导时不再显示', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(account: testAccount(), storedSession: true),
      localSettings: const LocalSettings(onboardingCompleted: true),
    );
    await pumpApp(tester, harness);
    expect(find.text('选择要自动备份的文件夹'), findsNothing);
    expect(find.text('全部文件'), findsWidgets);
  });
}
