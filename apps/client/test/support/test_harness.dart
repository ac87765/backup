import 'package:encrypted_backup_client/app.dart';
import 'package:encrypted_backup_client/core/clock.dart';
import 'package:encrypted_backup_client/core/config/app_config.dart';
import 'package:encrypted_backup_client/core/platform/external_links.dart';
import 'package:encrypted_backup_client/core/platform/file_picker.dart';
import 'package:encrypted_backup_client/core/platform/local_folders.dart';
import 'package:encrypted_backup_client/core/platform/window_controls.dart';
import 'package:encrypted_backup_client/core/providers/app_providers.dart';
import 'package:encrypted_backup_client/core/theme/app_theme.dart';
import 'package:encrypted_backup_client/desktop/onboarding/standard_folders.dart';
import 'package:encrypted_backup_client/desktop/settings/local_settings.dart';
import 'package:encrypted_backup_client/desktop/startup/startup_service.dart';
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
  String? directoryToPick;

  @override
  Future<List<String>> pickFiles() async => List<String>.of(filesToPick);

  @override
  Future<String?> pickSaveLocation(String suggestedName) async => savePath;

  @override
  Future<String?> pickDirectory() async => directoryToPick;
}

class FakeFolderInspector implements LocalFolderInspector {
  List<String> subfolders = <String>['合同', '设计稿', '临时导出'];

  @override
  Future<List<String>> listSubfolders(String path) async =>
      List<String>.of(subfolders);
}

class FakeStartupService implements StartupService {
  FakeStartupService({this.enabled = false});

  bool enabled;
  final List<bool> calls = <bool>[];

  @override
  Future<bool> isEnabled() async => enabled;

  @override
  Future<void> setEnabled(bool value) async {
    calls.add(value);
    enabled = value;
  }
}

class FakeWindowControls implements WindowControls {
  int hideCount = 0;
  int minimizeCount = 0;
  int maximizeCount = 0;
  int dragCount = 0;
  int showCount = 0;
  bool quitCalled = false;

  @override
  Future<void> hideToTray() async {
    hideCount += 1;
  }

  @override
  Future<void> minimize() async {
    minimizeCount += 1;
  }

  @override
  Future<void> quit() async {
    quitCalled = true;
  }

  @override
  Future<void> showAndFocus() async {
    showCount += 1;
  }

  @override
  Future<void> startDragging() async {
    dragCount += 1;
  }

  @override
  Future<void> toggleMaximize() async {
    maximizeCount += 1;
  }
}

class MemoryLocalSettingsStore implements LocalSettingsStore {
  MemoryLocalSettingsStore([this.settings = const LocalSettings()]);

  LocalSettings settings;
  final List<LocalSettings> saved = <LocalSettings>[];

  @override
  Future<LocalSettings> load() async => settings;

  @override
  Future<void> save(LocalSettings value) async {
    settings = value;
    saved.add(value);
  }
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
  TestHarness({
    FakeNativeBridge? bridge,
    LocalSettings? localSettings,
    List<StandardFolder>? standardFolders,
  }) : bridge = bridge ?? FakeNativeBridge(),
       settingsStore = MemoryLocalSettingsStore(
         localSettings ?? const LocalSettings(onboardingCompleted: true),
       ),
       standardFolders =
           standardFolders ??
           const <StandardFolder>[
             StandardFolder(
               name: '桌面',
               path: r'C:\Users\tlj\Desktop',
               kind: StandardFolderKind.desktop,
             ),
             StandardFolder(
               name: '文档',
               path: r'C:\Users\tlj\Documents',
               kind: StandardFolderKind.documents,
             ),
             StandardFolder(
               name: '图片',
               path: r'C:\Users\tlj\Pictures',
               kind: StandardFolderKind.pictures,
             ),
           ];

  final FakeNativeBridge bridge;
  final FakeFilePicker filePicker = FakeFilePicker();
  final FakeFolderInspector folderInspector = FakeFolderInspector();
  final FakeStartupService startup = FakeStartupService();
  final FakeWindowControls windowControls = FakeWindowControls();
  final MemoryLocalSettingsStore settingsStore;
  final List<StandardFolder> standardFolders;
  final FakeClock clock = FakeClock();
  final List<Uri> openedUris = <Uri>[];
  AppConfig config = testConfig;

  /// 与运行测试的主机无关：默认按桌面端（Windows / macOS）构建路由。
  bool desktop = true;

  List<Override> get overrides => <Override>[
    desktopPlatformProvider.overrideWithValue(desktop),
    nativeBridgeProvider.overrideWithValue(bridge),
    appConfigProvider.overrideWith((ref) async => config),
    bootstrapProvider.overrideWith((ref) async {}),
    clockProvider.overrideWithValue(clock.call),
    filePickerProvider.overrideWithValue(filePicker),
    localFolderInspectorProvider.overrideWithValue(folderInspector),
    localSettingsStoreProvider.overrideWith((ref) async => settingsStore),
    startupServiceProvider.overrideWithValue(startup),
    windowControlsProvider.overrideWithValue(windowControls),
    standardFoldersProvider.overrideWith((ref) async => standardFolders),
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
/// 设置 760×600、首次引导 760×720）。
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
