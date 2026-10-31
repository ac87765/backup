import 'package:encrypted_backup_client/app.dart';
import 'package:encrypted_backup_client/core/clock.dart';
import 'package:encrypted_backup_client/core/config/app_config.dart';
import 'package:encrypted_backup_client/core/platform/external_links.dart';
import 'package:encrypted_backup_client/core/platform/file_picker.dart';
import 'package:encrypted_backup_client/core/providers/app_providers.dart';
import 'package:encrypted_backup_client/core/theme/app_theme.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;
import 'package:flutter_test/flutter_test.dart';

import 'fake_native_bridge.dart';

const testConfig = AppConfig(
  serverUrl: 'https://120.79.245.99',
  appDataDir: r'C:\Temp\encrypted-backup-test',
  deviceName: 'DESKTOP-TLJ',
);

class FakeFilePicker implements FilePicker {
  List<String> filesToPick = <String>[];
  String? savePath;

  @override
  Future<List<String>> pickFiles() async => List<String>.of(filesToPick);

  @override
  Future<String?> pickSaveLocation(String suggestedName) async => savePath;
}

class FakeClock {
  FakeClock([DateTime? start])
    : current = start ?? DateTime(2026, 10, 31, 10, 5);

  DateTime current;

  void advance(Duration duration) {
    current = current.add(duration);
  }

  DateTime call() => current;
}

class TestHarness {
  TestHarness({FakeNativeBridge? bridge})
    : bridge = bridge ?? FakeNativeBridge();

  final FakeNativeBridge bridge;
  final FakeFilePicker filePicker = FakeFilePicker();
  final FakeClock clock = FakeClock();
  final List<Uri> openedUris = <Uri>[];
  AppConfig config = testConfig;

  List<Override> get overrides => <Override>[
    nativeBridgeProvider.overrideWithValue(bridge),
    appConfigProvider.overrideWith((ref) async => config),
    bootstrapProvider.overrideWith((ref) async {}),
    clockProvider.overrideWithValue(clock.call),
    filePickerProvider.overrideWithValue(filePicker),
    externalLinkOpenerProvider.overrideWithValue((uri) async {
      openedUris.add(uri);
      return true;
    }),
  ];

  Widget get app =>
      ProviderScope(overrides: overrides, child: const EncryptedBackupApp());

  Widget page(Widget home) => ProviderScope(
    overrides: overrides,
    child: RepaintBoundary(
      key: const Key('golden-root'),
      child: MaterialApp(
        debugShowCheckedModeBanner: false,
        theme: AppTheme.light,
        home: home,
      ),
    ),
  );
}

/// 固定测试窗口为设计稿逻辑尺寸（登录 440×600、主窗口 1280×800、
/// 设置 760×600）。
void setTestWindow(WidgetTester tester, Size size) {
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = size;
  addTearDown(tester.view.resetDevicePixelRatio);
  addTearDown(tester.view.resetPhysicalSize);
}

Future<void> pumpApp(WidgetTester tester, TestHarness harness) async {
  await tester.pumpWidget(harness.app);
  await tester.pumpAndSettle();
}
