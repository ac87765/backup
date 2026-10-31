import 'dart:convert';
import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/providers/app_providers.dart';

/// 桌面界面的本地设置（不属于 Rust 状态库）。
///
/// 只保存界面一次性的标记：首次引导是否完成、关闭到托盘提示是否展示过。
/// **不保存任何密钥、令牌或账号信息**；文件位于应用数据目录，与本地状态库
/// 同级。选择普通 JSON 文件而不是扩展桥接 API，是因为这些标记只影响界面，
/// Rust 核心不需要读取。
class LocalSettings {
  const LocalSettings({
    this.onboardingCompleted = false,
    this.closeHintShown = false,
  });

  final bool onboardingCompleted;
  final bool closeHintShown;

  LocalSettings copyWith({bool? onboardingCompleted, bool? closeHintShown}) {
    return LocalSettings(
      onboardingCompleted: onboardingCompleted ?? this.onboardingCompleted,
      closeHintShown: closeHintShown ?? this.closeHintShown,
    );
  }

  Map<String, Object?> toJson() => <String, Object?>{
    'onboardingCompleted': onboardingCompleted,
    'closeHintShown': closeHintShown,
  };

  static LocalSettings fromJson(Object? value) {
    if (value is! Map) {
      return const LocalSettings();
    }
    return LocalSettings(
      onboardingCompleted: value['onboardingCompleted'] == true,
      closeHintShown: value['closeHintShown'] == true,
    );
  }
}

abstract class LocalSettingsStore {
  Future<LocalSettings> load();

  Future<void> save(LocalSettings settings);
}

/// 生产实现：应用数据目录下的 `desktop-settings.json`。
class FileLocalSettingsStore implements LocalSettingsStore {
  FileLocalSettingsStore(String appDataDir)
    : _file = File('$appDataDir${Platform.pathSeparator}desktop-settings.json');

  final File _file;

  @override
  Future<LocalSettings> load() async {
    try {
      if (!await _file.exists()) {
        return const LocalSettings();
      }
      return LocalSettings.fromJson(jsonDecode(await _file.readAsString()));
    } catch (_) {
      // 文件损坏时按默认值处理；不记录路径或内容。
      return const LocalSettings();
    }
  }

  @override
  Future<void> save(LocalSettings settings) async {
    await _file.parent.create(recursive: true);
    await _file.writeAsString(jsonEncode(settings.toJson()), flush: true);
  }
}

final localSettingsStoreProvider = FutureProvider<LocalSettingsStore>((
  ref,
) async {
  final config = await ref.watch(appConfigProvider.future);
  return FileLocalSettingsStore(config.appDataDir);
});

/// 已加载的本地设置；`null` 表示仍在读取。
class LocalSettingsController extends Notifier<LocalSettings?> {
  @override
  LocalSettings? build() {
    Future<void>.microtask(_load);
    return null;
  }

  Future<void> _load() async {
    try {
      final store = await ref.read(localSettingsStoreProvider.future);
      final settings = await store.load();
      if (!ref.mounted) {
        return;
      }
      state = settings;
    } catch (_) {
      if (!ref.mounted) {
        return;
      }
      // 读不到时按“已完成引导”处理，避免把用户困在引导页。
      state = const LocalSettings(onboardingCompleted: true);
    }
  }

  Future<void> markOnboardingCompleted() =>
      _save((current) => current.copyWith(onboardingCompleted: true));

  Future<void> markCloseHintShown() =>
      _save((current) => current.copyWith(closeHintShown: true));

  Future<void> _save(LocalSettings Function(LocalSettings) update) async {
    final current = state ?? const LocalSettings();
    final next = update(current);
    state = next;
    try {
      final store = await ref.read(localSettingsStoreProvider.future);
      await store.save(next);
    } catch (_) {
      // 标记写入失败只影响下一次提示，不阻塞当前操作。
    }
  }
}

final localSettingsProvider =
    NotifierProvider<LocalSettingsController, LocalSettings?>(
      LocalSettingsController.new,
    );

/// 首次引导是否需要展示（加载完成且未完成引导）。
final onboardingRequiredProvider = Provider<bool>((ref) {
  final settings = ref.watch(localSettingsProvider);
  return settings != null && !settings.onboardingCompleted;
});
