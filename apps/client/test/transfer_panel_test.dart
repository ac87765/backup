import 'package:encrypted_backup_client/core/rust/models.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fake_native_bridge.dart';
import 'support/test_harness.dart';

TransferProgressDto progress({
  required String id,
  required TransferStatusDto status,
  int completed = 5 * 1000 * 1000,
  int total = 10 * 1000 * 1000,
  TransferFailureDto? error,
}) {
  return TransferProgressDto(
    transferId: id,
    direction: TransferDirectionDto.upload,
    status: status,
    completedBytes: completed,
    totalBytes: total,
    completedChunks: 5,
    totalChunks: 10,
    error: error,
  );
}

void main() {
  testWidgets('传输暂停 / 继续 / 取消与限流提示', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(
        account: testAccount(),
        storedSession: true,
        rootItems: [testNode(id: 'a', name: '已有文件.pdf')],
      ),
    );
    harness.filePicker.filesToPick = [r'C:\Users\tlj\报告.pdf'];
    await pumpApp(tester, harness);

    // 「上传」是下拉菜单（上传文件 / 上传文件夹）。
    await tester.tap(find.byKey(const Key('upload-menu-button')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey<String>('menu-item-上传文件')));
    await tester.pumpAndSettle();
    const id = 'transfer-1';
    harness.bridge.emitProgress(
      progress(id: id, status: TransferStatusDto.uploading),
    );
    // 事件经广播流异步送达：先让监听器处理，再渲染一帧。
    await tester.pump();
    await tester.pump();
    await tester.tap(find.byKey(const Key('open-transfers')));
    await tester.pumpAndSettle();

    expect(find.text('报告.pdf'), findsOneWidget);
    expect(find.textContaining('上传中'), findsWidgets);

    await tester.tap(find.byKey(const ValueKey<String>('transfer-pause-$id')));
    await tester.pumpAndSettle();
    expect(harness.bridge.calls, contains('pauseTransfer:$id'));

    harness.bridge.emitProgress(
      progress(id: id, status: TransferStatusDto.paused),
    );
    // 事件经广播流异步送达：先让监听器处理，再渲染一帧。
    await tester.pump();
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey<String>('transfer-resume-$id')));
    await tester.pumpAndSettle();
    expect(harness.bridge.calls, contains('resumeTransfer:$id'));

    harness.bridge.emitProgress(
      progress(id: id, status: TransferStatusDto.retrying),
    );
    // 事件经广播流异步送达：先让监听器处理，再渲染一帧。
    await tester.pump();
    await tester.pump();
    expect(find.textContaining('服务器繁忙'), findsWidgets);

    await tester.tap(find.byKey(const ValueKey<String>('transfer-cancel-$id')));
    await tester.pumpAndSettle();
    expect(harness.bridge.calls, contains('cancelTransfer:$id'));
  });

  testWidgets('失败原因与重试', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final harness = TestHarness(
      bridge: FakeNativeBridge(
        account: testAccount(),
        storedSession: true,
        rootItems: [testNode(id: 'a', name: '已有文件.pdf')],
      ),
    );
    harness.filePicker.filesToPick = [r'C:\Users\tlj\扫描件.pdf'];
    await pumpApp(tester, harness);

    // 「上传」是下拉菜单（上传文件 / 上传文件夹）。
    await tester.tap(find.byKey(const Key('upload-menu-button')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey<String>('menu-item-上传文件')));
    await tester.pumpAndSettle();
    const id = 'transfer-1';
    harness.bridge.emitProgress(
      progress(
        id: id,
        status: TransferStatusDto.failed,
        error: const TransferFailureDto(
          code: 'file_changed',
          message: '文件在上传过程中被修改',
        ),
      ),
    );
    // 事件经广播流异步送达：先让监听器处理，再渲染一帧。
    await tester.pump();
    await tester.pump();
    await tester.tap(find.byKey(const Key('open-transfers')));
    await tester.pumpAndSettle();

    expect(find.textContaining('文件在上传过程中被修改'), findsOneWidget);
    await tester.tap(find.byKey(const ValueKey<String>('transfer-retry-$id')));
    await tester.pumpAndSettle();
    expect(harness.bridge.calls, contains('retryTransfer:$id'));
  });

  testWidgets('重启后的待续传上传可以续传或放弃', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final bridge = FakeNativeBridge(
      account: testAccount(),
      storedSession: true,
    );
    bridge.pending = [
      const PendingUploadDto(
        sessionId: 'session-1',
        parentId: 'root',
        nodeId: 'node-1',
        contentId: 'content-1',
        versionId: 'version-1',
        localPath: r'C:\Users\tlj\大文件.bin',
        fileSize: 9000000,
        fileMtimeNs: 1759276800000000000,
        chunkCount: 1,
        declaredSize: 9000000,
        status: PendingUploadStatusDto.paused,
      ),
    ];
    final harness = TestHarness(bridge: bridge);
    await pumpApp(tester, harness);
    await tester.tap(find.byKey(const Key('open-transfers')));
    await tester.pumpAndSettle();

    expect(find.text('大文件.bin'), findsOneWidget);
    expect(find.textContaining('待续传'), findsWidgets);
    await tester.tap(
      find.byKey(const ValueKey<String>('pending-resume-pending:session-1')),
    );
    await tester.pumpAndSettle();
    expect(bridge.calls, contains('resumePendingUpload:session-1'));
  });

  testWidgets('待续传上传可以放弃', (tester) async {
    setTestWindow(tester, const Size(1280, 800));
    final bridge = FakeNativeBridge(
      account: testAccount(),
      storedSession: true,
    );
    bridge.pending = [
      const PendingUploadDto(
        sessionId: 'session-2',
        parentId: null,
        nodeId: 'node-2',
        contentId: 'content-2',
        versionId: 'version-2',
        localPath: r'C:\Users\tlj\旧文件.bin',
        fileSize: 1000,
        fileMtimeNs: 1759276800000000000,
        chunkCount: 0,
        declaredSize: 1000,
        status: PendingUploadStatusDto.failed,
      ),
    ];
    final harness = TestHarness(bridge: bridge);
    await pumpApp(tester, harness);
    await tester.tap(find.byKey(const Key('open-transfers')));
    await tester.pumpAndSettle();

    await tester.tap(
      find.byKey(const ValueKey<String>('pending-abandon-pending:session-2')),
    );
    await tester.pumpAndSettle();
    expect(bridge.calls, contains('abortPendingUpload:session-2'));
  });
}
