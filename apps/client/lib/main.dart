import 'package:encrypted_backup_client/app.dart';
import 'package:encrypted_backup_client/desktop/window/desktop_window.dart';
import 'package:encrypted_backup_client/src/rust/frb_generated.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  // 桥接层初始化（flutter_rust_bridge 的生成入口）。
  await RustLib.init();
  await configureDesktopWindow();
  runApp(const ProviderScope(child: EncryptedBackupApp()));
}
