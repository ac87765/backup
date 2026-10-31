import 'package:encrypted_backup_client/desktop/dialogs/restore_folder_dialog.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import '../support/fake_native_bridge.dart';
import '../support/test_harness.dart';

class _Launcher extends StatelessWidget {
  const _Launcher();

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: Center(
        child: Builder(
          builder: (buttonContext) => TextButton(
            onPressed: () => showRestoreFolderDialog(
              context: buttonContext,
              folderId: 'folder-1',
              folderName: '照片 / 2024',
              detail: '2,140 个文件 · 8.7 GB · 含 14 个子文件夹',
            ),
            child: const Text('打开'),
          ),
        ),
      ),
    );
  }
}

void main() {
  testWidgets('恢复到电脑默认保留两份并调用桥接', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(account: testAccount(), storedSession: true),
    );
    harness.filePicker.directoryToPick = r'D:\恢复';
    await tester.pumpWidget(harness.page(const _Launcher()));
    await tester.pumpAndSettle();

    await tester.tap(find.text('打开'));
    await tester.pumpAndSettle();
    expect(find.text('下载文件夹到电脑'), findsOneWidget);
    expect(find.textContaining('2,140 个文件'), findsOneWidget);

    await tester.tap(find.byKey(const Key('restore-pick-destination')));
    await tester.pumpAndSettle();
    expect(find.textContaining(r'D:\恢复'), findsWidgets);

    await tester.tap(find.byKey(const Key('restore-start')));
    await tester.pumpAndSettle();

    expect(
      harness.bridge.calls,
      contains(r'startDownloadFolder:folder-1:D:\恢复:keepBoth'),
    );
  });

  testWidgets('选择替换策略后传给桥接', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(account: testAccount(), storedSession: true),
    );
    harness.filePicker.directoryToPick = r'D:\恢复';
    await tester.pumpWidget(harness.page(const _Launcher()));
    await tester.pumpAndSettle();

    await tester.tap(find.text('打开'));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('restore-pick-destination')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const Key('restore-conflict-replace')));
    await tester.pump();
    await tester.tap(find.byKey(const Key('restore-start')));
    await tester.pumpAndSettle();

    expect(
      harness.bridge.calls,
      contains(r'startDownloadFolder:folder-1:D:\恢复:replace'),
    );
  });
}
