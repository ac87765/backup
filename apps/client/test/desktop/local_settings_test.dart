import 'dart:io';

import 'package:encrypted_backup_client/desktop/settings/local_settings.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('首次引导完成标记写入本地 JSON 并可读回', () async {
    final directory = await Directory.systemTemp.createTemp('backup-settings');
    addTearDown(() => directory.delete(recursive: true));
    final store = FileLocalSettingsStore(directory.path);

    expect((await store.load()).onboardingCompleted, isFalse);
    await store.save(
      const LocalSettings(onboardingCompleted: true, closeHintShown: true),
    );

    final loaded = await store.load();
    expect(loaded.onboardingCompleted, isTrue);
    expect(loaded.closeHintShown, isTrue);
  });

  test('文件内容损坏时回落到默认值', () async {
    final directory = await Directory.systemTemp.createTemp('backup-settings');
    addTearDown(() => directory.delete(recursive: true));
    await File(
      '${directory.path}${Platform.pathSeparator}desktop-settings.json',
    ).writeAsString('not json');
    final settings = await FileLocalSettingsStore(directory.path).load();
    expect(settings.onboardingCompleted, isFalse);
    expect(settings.closeHintShown, isFalse);
  });
}
