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
import 'package:encrypted_backup_client/desktop/lifecycle/app_quit.dart';
import 'package:encrypted_backup_client/desktop/settings/local_settings.dart';
import 'package:encrypted_backup_client/desktop/startup/startup_service.dart';
import 'package:encrypted_backup_client/desktop/state/update_controller.dart';
import 'package:flutter/foundation.dart' show ValueListenable;
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
  final List<WindowMode> modes = <WindowMode>[];
  final ValueNotifier<bool> _visible = ValueNotifier<bool>(true);

  @override
  ValueListenable<bool> get visible => _visible;

  @override
  Future<void> applyMode(WindowMode mode) async {
    modes.add(mode);
  }

  @override
  Future<void> hideToTray() async {
    hideCount += 1;
    _visible.value = false;
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
    _visible.value = true;
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

class FakeAppQuit implements AppQuit {
  int quitCalls = 0;
  TrayHider? trayHider;

  @override
  void registerTrayHider(TrayHider hider) {
    trayHider = hider;
  }

  @override
  Future<void> quit() async {
    quitCalls += 1;
  }
}

/// 可手动触发的更新计时器：测试不留下真实 Timer，也不依赖 24 小时真实时间。
class FakeUpdateScheduler implements UpdateScheduler {
  Duration? nextDelay;
  void Function()? _callback;

  bool get hasPending => _callback != null;

  @override
  void schedule(Duration delay, void Function() callback) {
    nextDelay = delay;
    _callback = callback;
  }

  @override
  void cancel() {
    _callback = null;
  }

  void fire() {
    final callback = _callback;
    _callback = null;
    callback?.call();
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
  final FakeAppQuit appQuit = FakeAppQuit();
  final FakeUpdateScheduler updateScheduler = FakeUpdateScheduler();
  final MemoryLocalSettingsStore settingsStore;
  final List<StandardFolder> standardFolders;
  final FakeClock clock = FakeClock();
  final List<Uri> openedUris = <Uri>[];
  final List<String> openedFolders = <String>[];
  bool openFolderResult = true;
  final Set<String> missingDirectories = <String>{};
  AppConfig config = testConfig;

  /// 与设计稿一致的关于页版本 / 提交；golden 不依赖构建时的 --dart-define。
  String appVersion = '0.1.0';
  String gitCommit = '7d82ea1';

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
    directoryExistsProvider.overrideWithValue(
      (path) => !missingDirectories.contains(path),
    ),
    localSettingsStoreProvider.overrideWith((ref) async => settingsStore),
    startupServiceProvider.overrideWithValue(startup),
    windowControlsProvider.overrideWithValue(windowControls),
    appQuitProvider.overrideWithValue(appQuit),
    updateSchedulerProvider.overrideWithValue(updateScheduler),
    appVersionProvider.overrideWithValue(appVersion),
    gitCommitProvider.overrideWithValue(gitCommit),
    standardFoldersProvider.overrideWith((ref) async => standardFolders),
    externalLinkOpenerProvider.overrideWithValue((uri) async {
      openedUris.add(uri);
      return true;
    }),
    localFolderOpenerProvider.overrideWithValue((path) async {
      openedFolders.add(path);
      return openFolderResult;
    }),
  ];

  Widget get app =>
      ProviderScope(overrides: overrides, child: const EncryptedBackupApp());

  /// 直接渲染单个页面（不经路由）。默认按主窗口模式（显示最大化按钮），
  /// 避免设置页等独立页面的标题栏与设计稿不一致。
  Widget page(Widget home, {WindowMode windowMode = WindowMode.main}) =>
      ProviderScope(
        overrides: <Override>[
          ...overrides,
          windowModeProvider.overrideWith(
            () => _FixedWindowModeController(windowMode),
          ),
        ],
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

class _FixedWindowModeController extends WindowModeController {
  _FixedWindowModeController(this._mode);

  final WindowMode _mode;

  @override
  WindowMode build() => _mode;
}

/// 固定测试窗口为设计稿逻辑尺寸（认证 880×560、主窗口 1280×800、
/// 设置 760×600、首次引导 760×720）。
///
/// 默认按系统“减少动态效果”运行：循环动画停止、进入动效直接完成，这样
/// `pumpAndSettle` 不会被无限动画卡住，golden 也保持确定。需要验证动效的
/// 测试传 `reduceMotion: false` 并自行 `pump`。
void setTestWindow(WidgetTester tester, Size size, {bool reduceMotion = true}) {
  tester.view.devicePixelRatio = 1;
  tester.view.physicalSize = size;
  addTearDown(tester.view.resetDevicePixelRatio);
  addTearDown(tester.view.resetPhysicalSize);
  final dispatcher = tester.binding.platformDispatcher;
  dispatcher.accessibilityFeaturesTestValue = FakeAccessibilityFeatures(
    disableAnimations: reduceMotion,
  );
  addTearDown(dispatcher.clearAccessibilityFeaturesTestValue);
}

Future<void> pumpApp(WidgetTester tester, TestHarness harness) async {
  await tester.pumpWidget(harness.app);
  await tester.pumpAndSettle();
}
