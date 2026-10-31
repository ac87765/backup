import 'dart:io';

import 'package:encrypted_backup_client/app.dart';
import 'package:encrypted_backup_client/core/platform/window_controls.dart';
import 'package:encrypted_backup_client/desktop/startup/startup_service.dart';
import 'package:encrypted_backup_client/desktop/window/desktop_runtime.dart';
import 'package:encrypted_backup_client/desktop/window/desktop_window.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_riverpod/misc.dart' show Override;

Future<void> main(List<String> args) async {
  WidgetsFlutterBinding.ensureInitialized();
  final desktop = Platform.isWindows || Platform.isMacOS;
  final startHidden = args.contains('--background');
  if (desktop) {
    // 开机启动（--background）只创建窗口与托盘，不显示主窗口。
    await configureDesktopWindow(startHidden: startHidden);
  }
  runApp(
    ProviderScope(
      overrides: desktop
          ? <Override>[
              windowControlsProvider.overrideWithValue(
                const DesktopWindowControls(),
              ),
              startupServiceProvider.overrideWithValue(
                LaunchAtStartupService(),
              ),
            ]
          : const <Override>[],
      child: desktop
          ? const DesktopRuntime(child: EncryptedBackupApp())
          : const EncryptedBackupApp(),
    ),
  );
}
