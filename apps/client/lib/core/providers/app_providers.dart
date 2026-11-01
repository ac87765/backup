import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../config/app_config.dart';
import '../rust/models.dart';
import '../rust/native_bridge.dart';

/// 桥接 API 的单一入口（测试可 override 成假实现）。
final nativeBridgeProvider = Provider<NativeBridge>((ref) {
  return const NativeBridge();
});

/// 编译期版本号 / 提交号；测试与 golden 可覆盖为设计稿中的值。
final appVersionProvider = Provider<String>((ref) => appVersion);
final gitCommitProvider = Provider<String>((ref) => gitCommit);

/// 启动配置（服务器地址、应用数据目录、设备名）。桌面入口启动时读取。
final appConfigProvider = FutureProvider<AppConfig>((ref) {
  return AppConfig.load();
});

/// 初始化 Rust 核心；界面需要调用桥接前先 await 这个 provider。
///
/// `ensureInitialized` 载入 flutter_rust_bridge 动态库，`initialize` 传入
/// 本地状态库目录、服务器地址与设备名（不登录、不发网络请求）。
final bootstrapProvider = FutureProvider<void>((ref) async {
  final config = await ref.watch(appConfigProvider.future);
  final bridge = ref.watch(nativeBridgeProvider);
  await bridge.ensureInitialized();
  await bridge.initialize(
    appDataDir: config.appDataDir,
    serverUrl: config.serverUrl,
    deviceName: config.deviceName,
    appVersion: appVersion,
  );
});

/// 单文件传输进度事件。
final transferProgressProvider = StreamProvider<TransferProgressDto>((ref) {
  return ref.watch(nativeBridgeProvider).transferProgressStream();
});

/// 文件夹任务进度事件。
final folderProgressProvider = StreamProvider<FolderProgressDto>((ref) {
  return ref.watch(nativeBridgeProvider).folderProgressStream();
});

/// 备份引擎状态/活动/汇总事件。
final backupEventsProvider = StreamProvider<BackupEventDto>((ref) {
  return ref.watch(nativeBridgeProvider).backupEventsStream();
});

/// 全局备份汇总（托盘/状态栏）。
final backupSummaryProvider = FutureProvider<BackupSummaryDto>((ref) async {
  await ref.watch(bootstrapProvider.future);
  return ref.watch(nativeBridgeProvider).backupSummary();
});

/// 全局备份设置（隐藏文件、上传限速、全局暂停）；设置页读取/写回。
final backupSettingsProvider = FutureProvider<BackupSettingsDto>((ref) async {
  await ref.watch(bootstrapProvider.future);
  return ref.watch(nativeBridgeProvider).backupSettings();
});

/// 已用 / 预留 / 配额；登录后加载，写操作后 invalidate。
final usageProvider = FutureProvider<UsageDto>((ref) async {
  await ref.watch(bootstrapProvider.future);
  return ref.watch(nativeBridgeProvider).getUsage();
});
