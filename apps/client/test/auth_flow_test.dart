import 'package:encrypted_backup_client/core/rust/models.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_native_bridge.dart';
import 'support/test_harness.dart';

void main() {
  testWidgets('登录成功后进入文件页', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness();
    await pumpApp(tester, harness);

    expect(find.text('登录加密备份'), findsOneWidget);
    await tester.enterText(
      find.byKey(const Key('login-email')),
      'tlj978626571@gmail.com',
    );
    await tester.enterText(find.byKey(const Key('login-password')), 'secret');
    await tester.tap(find.byKey(const Key('login-submit')));
    await tester.pumpAndSettle();

    expect(find.text('全部文件'), findsWidgets);
    expect(harness.bridge.calls, contains('login:tlj978626571@gmail.com'));
  });

  testWidgets('登录失败显示不区分账号是否存在的中文提示', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness();
    harness.bridge.loginError = const BridgeError(
      code: 'invalid_credentials',
      message: '邮箱或密码不正确',
    );
    await pumpApp(tester, harness);

    await tester.enterText(
      find.byKey(const Key('login-email')),
      'nobody@example.com',
    );
    await tester.enterText(find.byKey(const Key('login-password')), 'wrong');
    await tester.tap(find.byKey(const Key('login-submit')));
    await tester.pumpAndSettle();

    expect(find.text('邮箱或密码不正确'), findsOneWidget);
    expect(find.text('登录加密备份'), findsOneWidget);
  });

  testWidgets('待审核账号显示等待页且没有上传入口', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(
        account: testAccount(status: AccountStatusDto.pending),
        storedSession: true,
      ),
    );
    await pumpApp(tester, harness);

    expect(find.text('正在等待管理员审核'), findsOneWidget);
    expect(find.textContaining('tlj978626571@gmail.com'), findsWidgets);
    expect(find.text('上传'), findsNothing);
    expect(find.text('新建文件夹'), findsNothing);
  });

  testWidgets('待审核账号审核通过后可重新检查进入文件页', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(
        account: testAccount(status: AccountStatusDto.pending),
        storedSession: true,
      ),
    );
    await pumpApp(tester, harness);
    harness.bridge.account = testAccount();
    await tester.tap(find.byKey(const Key('pending-refresh')));
    await tester.pumpAndSettle();

    expect(find.text('全部文件'), findsWidgets);
  });

  testWidgets('解锁页显示邮箱，输入密码后进入文件页', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(
        account: testAccount(unlocked: false),
        storedSession: true,
      ),
    );
    harness.bridge.unlockAccount = testAccount();
    await pumpApp(tester, harness);

    expect(find.text('欢迎回来'), findsOneWidget);
    expect(find.text('tlj978626571@gmail.com'), findsOneWidget);
    await tester.enterText(find.byKey(const Key('unlock-password')), 'secret');
    await tester.tap(find.byKey(const Key('unlock-submit')));
    await tester.pumpAndSettle();

    expect(find.text('全部文件'), findsWidgets);
    expect(harness.bridge.calls, contains('unlock'));
  });

  testWidgets('解锁页切换账号会清除凭据并回到登录页', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(
        account: testAccount(unlocked: false),
        storedSession: true,
      ),
    );
    await pumpApp(tester, harness);

    await tester.tap(find.byKey(const Key('switch-account')));
    await tester.pumpAndSettle();

    expect(find.text('登录加密备份'), findsOneWidget);
    expect(harness.bridge.calls, contains('forgetCredentials'));
  });

  testWidgets('注册与忘记密码打开网页版的实际路由', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness();
    await pumpApp(tester, harness);

    await tester.tap(find.byKey(const Key('login-register')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('login-forgot')));
    await tester.pump();

    expect(harness.openedUris, hasLength(2));
    expect(harness.openedUris[0].path, '/register');
    // Web 的忘记密码路由是 /reset（web/src/App.tsx），不是 /forgot-password。
    expect(harness.openedUris[1].path, '/reset');
  });
}
