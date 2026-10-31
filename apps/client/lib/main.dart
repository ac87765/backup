import 'package:encrypted_backup_client/app.dart';
import 'package:encrypted_backup_client/desktop/window/desktop_window.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await configureDesktopWindow();
  runApp(const ProviderScope(child: EncryptedBackupApp()));
}
