import 'dart:async';

import 'package:encrypted_backup_client/common/pages/login_page.dart';
import 'package:encrypted_backup_client/common/widgets/auth_brand_panel.dart';
import 'package:encrypted_backup_client/core/rust/models.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/fake_native_bridge.dart';
import '../support/test_harness.dart';

void main() {
  testWidgets('品牌面板显示字标、标题、插画要点，且宽度为 400', (tester) async {
    setTestWindow(tester, const Size(880, 560));
    final harness = TestHarness();
    await pumpApp(tester, harness);
    await tester.pump();

    final panel = find.byType(AuthBrandPanel);
    expect(panel, findsOneWidget);
    expect(tester.getSize(panel).width, 400);
    expect(find.text('加密备份'), findsWidgets);
    expect(find.textContaining('只有你能打开'), findsOneWidget);
    expect(find.textContaining('文件在本机加密后才上传'), findsOneWidget);
    expect(find.text('在本机加密，密钥不离开这台电脑'), findsOneWidget);
    expect(find.text('文件夹自动增量备份，保留旧版本'), findsOneWidget);
    expect(find.text('服务器无法看到你的密码和文件内容'), findsOneWidget);
  });

  testWidgets('减少动态效果时品牌面板没有循环动画、不显示流动方块', (tester) async {
    setTestWindow(tester, const Size(400, 528));
    final harness = TestHarness();
    await tester.pumpWidget(
      harness.page(
        const SizedBox(width: 400, height: 528, child: AuthBrandPanel()),
      ),
    );
    await tester.pump();

    expect(find.byKey(const ValueKey<String>('auth-packet-a')), findsNothing);
    expect(find.byKey(const ValueKey<String>('auth-packet-b')), findsNothing);
    expect(find.byKey(const ValueKey<String>('auth-packet-c')), findsNothing);
    expect(find.byKey(const ValueKey<String>('auth-packet-d')), findsNothing);
    // 循环控制器已停止，不再有排队中的帧。
    expect(tester.binding.hasScheduledFrame, isFalse);
  });

  testWidgets('开启动效时品牌面板循环播放并显示流动方块', (tester) async {
    setTestWindow(tester, const Size(400, 528), reduceMotion: false);
    final harness = TestHarness();
    await tester.pumpWidget(
      harness.page(
        const SizedBox(width: 400, height: 528, child: AuthBrandPanel()),
      ),
    );
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 16));

    expect(find.byKey(const ValueKey<String>('auth-packet-a')), findsOneWidget);
    expect(find.byKey(const ValueKey<String>('auth-packet-c')), findsOneWidget);
    expect(tester.binding.hasScheduledFrame, isTrue);

    await tester.pumpWidget(const SizedBox.shrink());
    await tester.pump();
  });

  testWidgets('减少动态效果时 working 状态不绘制按钮进度条', (tester) async {
    setTestWindow(tester, const Size(880, 560));
    final completer = Completer<AccountDto>();
    final harness = TestHarness();
    harness.bridge.loginCompleter = completer;
    await pumpApp(tester, harness);

    expect(find.byType(LoginPage), findsOneWidget);
    await tester.enterText(
      find.byKey(const Key('login-email')),
      'tlj978626571@gmail.com',
    );
    await tester.enterText(find.byKey(const Key('login-password')), 'secret');
    await tester.tap(find.byKey(const Key('login-submit')));
    await tester.pump(const Duration(milliseconds: 50));

    expect(find.text('正在验证…'), findsOneWidget);
    expect(find.byKey(const Key('button-progress')), findsNothing);

    completer.complete(testAccount());
    await tester.pumpAndSettle();
  });
}
