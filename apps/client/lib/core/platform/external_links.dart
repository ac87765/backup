import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:url_launcher/url_launcher.dart';

/// 打开外部浏览器。抽成 provider 便于测试断言，不在测试里触达平台通道。
typedef ExternalLinkOpener = Future<bool> Function(Uri uri);

Future<bool> _openInBrowser(Uri uri) {
  return launchUrl(uri, mode: LaunchMode.externalApplication);
}

final externalLinkOpenerProvider = Provider<ExternalLinkOpener>((ref) {
  return _openInBrowser;
});

/// 在系统文件管理器中打开本地文件夹；文件夹不存在或平台不支持时返回 `false`。
typedef LocalFolderOpener = Future<bool> Function(String path);

/// 不走 url_launcher：Windows 的 ShellExecute 打不开百分号编码的 file URI，
/// 而应用数据目录含中文和空格。
Future<bool> _openLocalFolder(String path) async {
  if (!await Directory(path).exists()) {
    return false;
  }
  final String command;
  if (Platform.isWindows) {
    command = 'explorer.exe';
  } else if (Platform.isMacOS) {
    command = 'open';
  } else {
    return false;
  }
  await Process.start(command, [path], mode: ProcessStartMode.detached);
  return true;
}

final localFolderOpenerProvider = Provider<LocalFolderOpener>((ref) {
  return _openLocalFolder;
});

/// 拼接网页版路径（以 `web/src/App.tsx` 的路由为准）。
Uri webRoute(String serverUrl, String path) {
  final base = serverUrl.endsWith('/')
      ? serverUrl.substring(0, serverUrl.length - 1)
      : serverUrl;
  return Uri.parse('$base$path');
}
