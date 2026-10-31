import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:launch_at_startup/launch_at_startup.dart';

/// 开机自动启动。默认空实现，桌面入口在 `main.dart` 覆盖；
/// 组件测试使用假实现，不写注册表 / Login Items。
abstract class StartupService {
  Future<bool> isEnabled();

  Future<void> setEnabled(bool enabled);
}

class NoopStartupService implements StartupService {
  const NoopStartupService();

  @override
  Future<bool> isEnabled() async => false;

  @override
  Future<void> setEnabled(bool enabled) async {}
}

/// 生产实现：`launch_at_startup`。启动参数 `--background` 只启动到托盘。
class LaunchAtStartupService implements StartupService {
  LaunchAtStartupService() {
    launchAtStartup.setup(
      appName: '加密备份',
      appPath: Platform.resolvedExecutable,
      args: const <String>['--background'],
    );
  }

  @override
  Future<bool> isEnabled() => launchAtStartup.isEnabled();

  @override
  Future<void> setEnabled(bool enabled) async {
    if (enabled) {
      await launchAtStartup.enable();
    } else {
      await launchAtStartup.disable();
    }
  }
}

final startupServiceProvider = Provider<StartupService>((ref) {
  return const NoopStartupService();
});

/// 系统当前是否已登记开机启动。
final startupEnabledProvider = FutureProvider<bool>((ref) {
  return ref.watch(startupServiceProvider).isEnabled();
});
