import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_native_bridge.dart';
import 'support/test_harness.dart';

void main() {
  testWidgets('登录后显示主窗口骨架并可切换侧栏页面', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(
        account: testAccount(),
        storedSession: true,
        rootItems: [testNode(id: 'a', name: '合同.pdf')],
      ),
    );
    await pumpApp(tester, harness);

    for (final label in const ['全部文件', '最近', '回收站', '安全与隐私']) {
      expect(find.text(label), findsWidgets, reason: '侧栏缺少 $label');
    }
    expect(find.text('合同.pdf'), findsOneWidget);

    await tester.tap(find.text('回收站').first);
    await tester.pumpAndSettle();
    expect(find.text('回收站是空的'), findsOneWidget);

    await tester.tap(find.text('安全与隐私').first);
    await tester.pumpAndSettle();
    expect(find.text('你的文件，只有你能打开'), findsOneWidget);

    await tester.tap(find.text('最近').first);
    await tester.pumpAndSettle();
    expect(find.text('“最近”暂不可用'), findsOneWidget);

    await tester.tap(find.byKey(const Key('open-settings')));
    await tester.pumpAndSettle();
    expect(find.text('通用'), findsWidgets);
  });
}
