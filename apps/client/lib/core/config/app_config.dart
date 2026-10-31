import 'dart:io';

import 'package:path_provider/path_provider.dart';

/// 编译期版本信息；由 CI 的 `--dart-define` 注入，缺省为 `dev`。
const String appVersion = String.fromEnvironment(
  'APP_VERSION',
  defaultValue: 'dev',
);

/// 编译期提交号；由 CI 的 `--dart-define` 注入，缺省为 `dev`。
const String gitCommit = String.fromEnvironment(
  'GIT_COMMIT',
  defaultValue: 'dev',
);

/// 发布产物地址（可选）；未配置时“关于”页禁用下载按钮。
const String releasesUrl = String.fromEnvironment('RELEASES_URL');

/// 应用启动配置（服务器地址、本地状态库目录、设备名）。
///
/// - 服务器地址：编译期 `String.fromEnvironment('SERVER_URL')`，缺省为生产
///   地址（与 Web 部署一致）；
/// - 应用数据目录：`path_provider` 的 application support 目录（Windows
///   `%APPDATA%`、macOS `~/Library/Application Support` 下由系统分配）；
/// - 设备名：系统主机名；macOS 按设计稿显示为“某某的 MacBook Pro”。
class AppConfig {
  const AppConfig({
    required this.serverUrl,
    required this.appDataDir,
    required this.deviceName,
  });

  final String serverUrl;
  final String appDataDir;
  final String deviceName;

  /// 读取运行环境配置。桌面入口在启动时 await 一次，测试直接构造。
  static Future<AppConfig> load() async {
    const serverUrl = String.fromEnvironment(
      'SERVER_URL',
      defaultValue: 'https://120.79.245.99',
    );
    final support = await getApplicationSupportDirectory();
    return AppConfig(
      serverUrl: serverUrl,
      appDataDir: support.path,
      deviceName: formatDeviceName(
        Platform.localHostname,
        isMacOS: Platform.isMacOS,
      ),
    );
  }

  /// 把系统主机名格式化为界面展示用设备名。
  ///
  /// Windows 保持大写主机名（如 `DESKTOP-TLJ`）；macOS 的 Bonjour 主机名
  ///（如 `TLJ-MacBook-Pro.local`）按 design/desktop 的写法显示为
  /// `TLJ 的 MacBook Pro`。
  static String formatDeviceName(String raw, {required bool isMacOS}) {
    var value = raw.trim();
    if (value.toLowerCase().endsWith('.local')) {
      value = value.substring(0, value.length - '.local'.length);
    }
    if (!isMacOS || value.isEmpty) {
      return value.isEmpty ? 'computer' : value;
    }
    value = value.replaceAll(RegExp(r'[-_]+'), ' ').trim();
    final model = RegExp(
      r'(MacBook(?: Pro| Air)?|iMac|Mac mini|Mac Pro|Mac Studio)',
    ).firstMatch(value);
    if (model != null && model.start > 0) {
      final owner = value.substring(0, model.start).trim();
      final rest = value.substring(model.start);
      return owner.isEmpty ? rest : '$owner 的 $rest';
    }
    return value;
  }
}
