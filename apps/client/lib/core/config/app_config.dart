import 'dart:io';

/// 应用启动配置（服务器地址、本地状态库目录、设备名）。
///
/// 服务器地址与设备名之后由“设置”页（第 5 组）持久化；本组先给可运行默认值：
/// - 服务器地址：编译期 `--dart-define=BACKUP_SERVER_URL=...`，否则本地开发地址；
/// - 应用数据目录：Windows `%APPDATA%\\EncryptedBackup`，macOS
///   `~/Library/Application Support/EncryptedBackup`；
/// - 设备名：系统主机名（对应 design D6 的默认云端路径）。
class AppConfig {
  const AppConfig({
    required this.serverUrl,
    required this.appDataDir,
    required this.deviceName,
  });

  final String serverUrl;
  final String appDataDir;
  final String deviceName;

  factory AppConfig.platformDefaults() {
    const override = String.fromEnvironment('BACKUP_SERVER_URL');
    final hostname = Platform.localHostname.trim();
    return AppConfig(
      serverUrl: override.isEmpty ? 'http://127.0.0.1:8080' : override,
      appDataDir: _defaultAppDataDir(),
      deviceName: hostname.isEmpty ? 'computer' : hostname,
    );
  }

  static String _defaultAppDataDir() {
    final environment = Platform.environment;
    if (Platform.isWindows) {
      final base = environment['APPDATA'] ?? environment['LOCALAPPDATA'];
      if (base != null && base.isNotEmpty) {
        return _join(base, const ['EncryptedBackup']);
      }
    } else if (Platform.isMacOS) {
      final home = environment['HOME'];
      if (home != null && home.isNotEmpty) {
        return _join(home, const [
          'Library',
          'Application Support',
          'EncryptedBackup',
        ]);
      }
    }
    final home = environment['HOME'] ?? environment['USERPROFILE'];
    if (home != null && home.isNotEmpty) {
      return _join(home, const ['.encrypted-backup']);
    }
    return _join(Directory.current.path, const ['encrypted-backup-data']);
  }

  static String _join(String base, List<String> parts) {
    return [base, ...parts].join(Platform.pathSeparator);
  }
}
