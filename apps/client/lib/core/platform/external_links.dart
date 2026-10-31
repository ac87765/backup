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

/// 拼接网页版路径（以 `web/src/App.tsx` 的路由为准）。
Uri webRoute(String serverUrl, String path) {
  final base = serverUrl.endsWith('/')
      ? serverUrl.substring(0, serverUrl.length - 1)
      : serverUrl;
  return Uri.parse('$base$path');
}
