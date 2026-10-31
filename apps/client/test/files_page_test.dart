import 'package:encrypted_backup_client/core/rust/models.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import 'package:flutter/gestures.dart' show kDoubleTapTimeout;
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_native_bridge.dart';
import 'support/test_harness.dart';

void main() {
  testWidgets('文件列表按名称排序，点击表头切换升/降序', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(
        account: testAccount(),
        storedSession: true,
        rootItems: [
          testNode(id: 'b', name: 'B 文件.pdf', size: 2000),
          testNode(id: 'a', name: 'A 文件.docx', size: 1000),
          testNode(id: 'folder', name: '合同', kind: NodeKindDto.folder),
        ],
      ),
    );
    harness.bridge.versions['a'] = [testVersion(id: 'v1', isCurrent: true)];
    await pumpApp(tester, harness);

    double topOf(String id) =>
        tester.getTopLeft(find.byKey(ValueKey<String>('file-row-$id'))).dy;

    // 文件夹始终在文件前；文件名升序。
    expect(topOf('folder'), lessThan(topOf('a')));
    expect(topOf('a'), lessThan(topOf('b')));

    await tester.tap(find.text('名称'));
    await tester.pumpAndSettle();

    expect(topOf('b'), lessThan(topOf('a')));
  });

  testWidgets('分页每次加载 50 项', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final items = <NodeDto>[
      for (var index = 0; index < 120; index += 1)
        testNode(
          id: 'node-$index',
          name: 'file-${index.toString().padLeft(3, '0')}.txt',
          size: 100 + index,
        ),
    ];
    final harness = TestHarness(
      bridge: FakeNativeBridge(
        account: testAccount(),
        storedSession: true,
        rootItems: items,
      ),
    );
    await pumpApp(tester, harness);

    expect(find.textContaining('已显示 50 / 120'), findsOneWidget);
    await tester.tap(find.byKey(const Key('load-more-files')));
    await tester.pumpAndSettle();
    expect(find.textContaining('已显示 100 / 120'), findsOneWidget);
  });

  testWidgets('列表 / 网格视图切换', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(
        account: testAccount(),
        storedSession: true,
        rootItems: [
          testNode(id: 'a', name: 'A.pdf', size: 1024),
          testNode(id: 'b', name: 'B.png', size: 2048),
        ],
      ),
    );
    await pumpApp(tester, harness);

    expect(find.byKey(const Key('file-grid')), findsNothing);
    await tester.tap(find.byIcon(LucideIcons.grid));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('file-grid')), findsOneWidget);
    await tester.tap(find.byIcon(LucideIcons.list));
    await tester.pumpAndSettle();
    expect(find.byKey(const Key('file-grid')), findsNothing);
  });

  testWidgets('重命名文件后列表刷新', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(
        account: testAccount(),
        storedSession: true,
        rootItems: [testNode(id: 'a', name: '旧名字.pdf', size: 1024)],
      ),
    );
    await pumpApp(tester, harness);

    await tester.tap(find.byKey(const ValueKey<String>('file-row-a')));
    await tester.pump(kDoubleTapTimeout);
    await tester.tap(find.byKey(const ValueKey<String>('file-more-a')));
    await tester.pump(kDoubleTapTimeout);    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey<String>('menu-item-重命名')));
    await tester.pumpAndSettle();

    await tester.enterText(
      find.byKey(const Key('name-dialog-input')),
      '新名字.pdf',
    );
    await tester.tap(find.text('保存'));
    await tester.pumpAndSettle();

    expect(find.text('新名字.pdf'), findsOneWidget);
    expect(harness.bridge.calls, contains('renameNode:a:新名字.pdf'));
  });

  testWidgets('删除到回收站并在回收站恢复', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(
        account: testAccount(),
        storedSession: true,
        rootItems: [testNode(id: 'a', name: '要删除.pdf', size: 1024)],
      ),
    );
    await pumpApp(tester, harness);

    await tester.tap(find.byKey(const ValueKey<String>('file-row-a')));
    await tester.pump(kDoubleTapTimeout);
    await tester.tap(find.byKey(const ValueKey<String>('file-more-a')));
    await tester.pump(kDoubleTapTimeout);    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey<String>('menu-item-移到回收站')));
    await tester.pumpAndSettle();

    expect(find.text('要删除.pdf'), findsNothing);
    expect(harness.bridge.calls, contains('deleteToTrash:a'));

    await tester.tap(find.text('回收站').first);
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey<String>('trash-item-a')), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey<String>('restore-a')));
    await tester.pumpAndSettle();
    expect(find.text('回收站是空的'), findsOneWidget);
    expect(harness.bridge.calls, contains('restoreFromTrash:a'));
  });

  testWidgets('版本历史列出当前版本并可恢复旧版本', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final bridge = FakeNativeBridge(
      account: testAccount(),
      storedSession: true,
      rootItems: [testNode(id: 'a', name: '合同.pdf', size: 1024)],
    );
    bridge.versions['a'] = [
      testVersion(id: 'v2', isCurrent: true, createdAt: '2026-10-31T09:00:00Z'),
      testVersion(id: 'v1', createdAt: '2026-10-01T09:00:00Z'),
    ];
    final harness = TestHarness(bridge: bridge);
    await pumpApp(tester, harness);

    await tester.tap(find.byKey(const ValueKey<String>('file-row-a')));
    await tester.pump(kDoubleTapTimeout);
    await tester.tap(find.byKey(const ValueKey<String>('file-more-a')));
    await tester.pump(kDoubleTapTimeout);    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey<String>('menu-item-版本历史')));
    await tester.pumpAndSettle();

    expect(find.text('版本历史 · 合同.pdf'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey<String>('restore-version-v1')));
    await tester.pumpAndSettle();
    expect(bridge.calls, contains('restoreVersion:a:v1'));
  });

  testWidgets('永久删除回收站项目需要二次确认', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final bridge = FakeNativeBridge(
      account: testAccount(),
      storedSession: true,
    );
    bridge.trash.add(
      TrashItemDto(
        node: testNode(id: 't1', name: '废弃.zip', size: 4096),
        deletedAt: '2026-10-30T09:00:00Z',
        trashBatchId: 'batch-1',
      ),
    );
    final harness = TestHarness(bridge: bridge);
    await pumpApp(tester, harness);

    await tester.tap(find.text('回收站').first);
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey<String>('permanent-t1')));
    await tester.pumpAndSettle();
    expect(find.text('永久删除'), findsWidgets);

    await tester.tap(find.text('取消'));
    await tester.pumpAndSettle();
    expect(bridge.calls, isNot(contains('permanentlyDelete:t1')));

    await tester.tap(find.byKey(const ValueKey<String>('permanent-t1')));
    await tester.pumpAndSettle();
    await tester.tap(find.text('永久删除').last);
    await tester.pumpAndSettle();
    expect(bridge.calls, contains('permanentlyDelete:t1'));
  });
}
