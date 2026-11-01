import '../../core/rust/models.dart';

/// 托盘图标状态（icon 只有这四种；错误状态复用“已暂停”图标）。
enum TrayIconState { normal, uploading, paused, needsUnlock }

/// 托盘菜单中的更新项：有新版本可点开“关于”，下载中显示进度并禁用。
class TrayUpdateEntry {
  const TrayUpdateEntry.available(this.version) : percent = null;

  const TrayUpdateEntry.downloading(this.percent) : version = null;

  final String? version;
  final int? percent;

  bool get enabled => version != null;

  String get label =>
      version != null ? '有新版本 $version…' : '正在下载更新 ${percent ?? 0}%';
}

/// 托盘菜单与图标的一次性展示模型。
///
/// 纯函数生成，便于测试状态 → 文案 / 图标映射；不读取 provider、不触碰
/// 平台通道。
class TrayMenuModel {
  const TrayMenuModel({
    required this.icon,
    required this.statusLabel,
    required this.pauseResumeLabel,
    required this.pauseResumeEnabled,
    required this.needsUnlock,
    required this.tooltip,
    this.update,
  });

  final TrayIconState icon;
  final String statusLabel;
  final String pauseResumeLabel;
  final bool pauseResumeEnabled;
  final bool needsUnlock;
  final String tooltip;

  /// 有新版本或正在下载时的额外菜单项；没有更新时为 null。
  final TrayUpdateEntry? update;
}

/// [needsUnlock] 来自认证阶段（有会话但内存里没有主密钥）；与备份汇总中的
/// “需要解锁”合并判断。[update] 来自更新状态；强制更新阻断时不传。
/// [backupBlocked] 为 true 时（强制更新）禁用暂停 / 恢复，引擎保持停止。
TrayMenuModel trayMenuModelFor(
  BackupSummaryDto? summary, {
  bool needsUnlock = false,
  bool backupBlocked = false,
  TrayUpdateEntry? update,
}) {
  if (backupBlocked) {
    return const TrayMenuModel(
      icon: TrayIconState.paused,
      statusLabel: '需要更新后才能继续使用',
      pauseResumeLabel: '暂停备份',
      pauseResumeEnabled: false,
      needsUnlock: false,
      tooltip: '需要更新后才能继续使用',
    );
  }
  if (needsUnlock || summary?.kind == BackupSummaryStatusKindDto.needsUnlock) {
    const status = '需要解锁才能继续备份';
    return TrayMenuModel(
      icon: TrayIconState.needsUnlock,
      statusLabel: status,
      pauseResumeLabel: '恢复备份',
      pauseResumeEnabled: false,
      needsUnlock: true,
      tooltip: status,
      update: update,
    );
  }
  if (summary == null) {
    return TrayMenuModel(
      icon: TrayIconState.paused,
      statusLabel: '备份状态不可用',
      pauseResumeLabel: '暂停备份',
      pauseResumeEnabled: false,
      needsUnlock: false,
      tooltip: '加密备份',
      update: update,
    );
  }
  switch (summary.kind) {
    case BackupSummaryStatusKindDto.uploading:
      final status = '正在备份 ${summary.done} / ${summary.total}';
      return TrayMenuModel(
        icon: TrayIconState.uploading,
        statusLabel: status,
        pauseResumeLabel: '暂停备份',
        pauseResumeEnabled: true,
        needsUnlock: false,
        tooltip: status,
        update: update,
      );
    case BackupSummaryStatusKindDto.paused:
      return TrayMenuModel(
        icon: TrayIconState.paused,
        statusLabel: '已暂停',
        pauseResumeLabel: '恢复备份',
        pauseResumeEnabled: true,
        needsUnlock: false,
        tooltip: '备份已暂停',
        update: update,
      );
    case BackupSummaryStatusKindDto.error:
      return TrayMenuModel(
        icon: TrayIconState.paused,
        statusLabel: '备份遇到问题',
        pauseResumeLabel: '暂停备份',
        pauseResumeEnabled: true,
        needsUnlock: false,
        tooltip: '备份遇到问题',
        update: update,
      );
    case BackupSummaryStatusKindDto.allBackedUp:
    case BackupSummaryStatusKindDto.needsUnlock:
      return TrayMenuModel(
        icon: TrayIconState.normal,
        statusLabel: '已全部备份',
        pauseResumeLabel: '暂停备份',
        pauseResumeEnabled: true,
        needsUnlock: false,
        tooltip: '加密备份 · 已全部备份',
        update: update,
      );
  }
}

/// 托盘资源路径（pubspec `assets/tray/`）。
///
/// macOS 使用黑色 + 透明的模板图（@2x），由系统按菜单栏深浅色自动着色。
String trayIconAsset(TrayIconState state, {required bool macOS}) {
  final name = switch (state) {
    TrayIconState.normal => 'normal',
    TrayIconState.uploading => 'uploading',
    TrayIconState.paused => 'paused',
    TrayIconState.needsUnlock => 'needs-unlock',
  };
  return macOS
      ? 'assets/tray/tray-${name}Template.png'
      : 'assets/tray/tray-$name.ico';
}
