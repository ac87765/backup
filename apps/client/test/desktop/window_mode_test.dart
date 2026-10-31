import 'package:encrypted_backup_client/app.dart';
import 'package:encrypted_backup_client/common/state/auth_controller.dart';
import 'package:encrypted_backup_client/core/platform/window_controls.dart';
import 'package:encrypted_backup_client/desktop/settings/local_settings.dart';
import 'package:flutter/gestures.dart' show kDoubleTapTimeout;
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/fake_native_bridge.dart';
import '../support/test_harness.dart';

void main() {
  testWidgets('未登录时窗口切到 auth 模式，标题栏不可最大化', (tester) async {
    setTestWindow(tester, const Size(880, 560));
    final harness = TestHarness();
    await pumpApp(tester, harness);
    await tester.pump();

    expect(harness.windowControls.modes, <WindowMode>[WindowMode.auth]);
    expect(find.byKey(const Key('caption-minimize')), findsOneWidget);
    expect(find.byKey(const Key('caption-close')), findsOneWidget);
    expect(find.byKey(const Key('caption-maximize')), findsNothing);

    // 双击标题栏不最大化。
    await tester.tap(find.byKey(const Key('window-titlebar')));
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(find.byKey(const Key('window-titlebar')));
    await tester.pump();
    expect(harness.windowControls.maximizeCount, 0);
  });

  testWidgets('登录成功进入主界面后窗口切到 main 模式', (tester) async {
    // 假的 WindowControls 不会真的改变测试视图尺寸：登录后进入主界面，用主窗口尺寸布局。
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness();
    await pumpApp(tester, harness);
    expect(harness.windowControls.modes, <WindowMode>[WindowMode.auth]);

    await tester.enterText(
      find.byKey(const Key('login-email')),
      'tlj978626571@gmail.com',
    );
    await tester.enterText(find.byKey(const Key('login-password')), 'secret');
    await tester.tap(find.byKey(const Key('login-submit')));
    await tester.pumpAndSettle();

    expect(harness.windowControls.modes, <WindowMode>[
      WindowMode.auth,
      WindowMode.main,
    ]);
    expect(find.byKey(const Key('caption-maximize')), findsOneWidget);

    // 主界面双击标题栏恢复最大化。
    await tester.tap(find.byKey(const Key('window-titlebar')));
    await tester.pump(const Duration(milliseconds: 50));
    await tester.tap(find.byKey(const Key('window-titlebar')));
    await tester.pump(kDoubleTapTimeout);
    expect(harness.windowControls.maximizeCount, 1);
  });

  testWidgets('首次引导切换 onboarding，完成后进入 main', (tester) async {
    // 假的 WindowControls 不会真的改变测试视图尺寸：登录后进入主界面，用主窗口尺寸布局。
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(account: testAccount(), storedSession: true),
      localSettings: const LocalSettings(),
    );
    await pumpApp(tester, harness);
    await tester.pump();

    // 启动页（auth）在引导之前是允许的；从 onboarding 开始必须是
    // onboarding → main。
    final initialModes = harness.windowControls.modes;
    expect(initialModes, contains(WindowMode.onboarding));

    await tester.tap(find.byKey(const Key('onboarding-skip')));
    await tester.pumpAndSettle();

    final modes = harness.windowControls.modes;
    final onboardingIndex = modes.indexOf(WindowMode.onboarding);
    expect(onboardingIndex, isNonNegative);
    expect(modes.sublist(onboardingIndex), <WindowMode>[
      WindowMode.onboarding,
      WindowMode.main,
    ]);
  });

  testWidgets('退出登录后窗口回到 auth 模式', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(account: testAccount(), storedSession: true),
    );
    await pumpApp(tester, harness);
    await tester.pump();
    expect(harness.windowControls.modes.last, WindowMode.main);

    final container = ProviderScope.containerOf(
      tester.element(find.byType(EncryptedBackupApp)),
      listen: false,
    );
    await container.read(authControllerProvider.notifier).logout();
    await tester.pumpAndSettle();

    expect(harness.windowControls.modes.last, WindowMode.auth);
    expect(find.byKey(const Key('login-title')), findsOneWidget);
  });

  testWidgets('窗口隐藏到托盘时停止界面 Ticker，重新显示后恢复', (tester) async {
    setTestWindow(tester, const Size(880, 560), reduceMotion: false);
    final harness = TestHarness();
    // 循环动画不会 settle，不能使用 pumpApp。
    await tester.pumpWidget(harness.app);
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 16));

    // 品牌面板的循环动画在可见时持续请求帧。
    expect(tester.binding.hasScheduledFrame, isTrue);

    await harness.windowControls.hideToTray();
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    expect(tester.binding.hasScheduledFrame, isFalse);

    await harness.windowControls.showAndFocus();
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 16));
    expect(tester.binding.hasScheduledFrame, isTrue);

    // 卸载界面，释放循环动画的 Ticker。
    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
  });
}
