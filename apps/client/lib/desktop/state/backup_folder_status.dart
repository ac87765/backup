import '../../core/rust/models.dart';

/// 状态胶囊色调（对应 tokens.css `.pill-*`）。
enum FolderStatusTone { ok, busy, paused, warn, needsUnlock }

/// 备份文件夹卡片的状态文案与进度，纯函数便于测试五种状态映射。
class FolderStatusView {
  const FolderStatusView({
    required this.label,
    required this.tone,
    this.progress,
    this.remaining = 0,
  });

  final String label;
  final FolderStatusTone tone;

  /// 0..1；仅“正在备份”时有值。
  final double? progress;

  /// 暂停时还会剩余多少文件（0 表示未知）。
  final int remaining;
}

FolderStatusView folderStatusView(
  BackupFolderDto folder,
  BackupFolderStateDto? state, {
  required bool globalPaused,
  bool needsUnlock = false,
}) {
  final status = state?.status;
  final done = status?.done ?? 0;
  final total = status?.total ?? 0;
  final remaining = total > done ? total - done : 0;
  final paused =
      globalPaused ||
      !folder.enabled ||
      state?.pauseReason == BackupPauseReasonDto.user ||
      status?.kind == BackupFolderStatusKindDto.paused;

  if (needsUnlock ||
      state?.pauseReason == BackupPauseReasonDto.needsUnlock ||
      status?.kind == BackupFolderStatusKindDto.needsUnlock) {
    return const FolderStatusView(
      label: '需要解锁',
      tone: FolderStatusTone.needsUnlock,
    );
  }
  if (paused) {
    return FolderStatusView(
      label: globalPaused && remaining > 0 ? '已暂停 · 剩余 $remaining 个' : '已暂停',
      tone: FolderStatusTone.paused,
      remaining: remaining,
    );
  }
  if (status == null) {
    return folder.enabled
        ? const FolderStatusView(label: '已备份', tone: FolderStatusTone.ok)
        : const FolderStatusView(label: '已暂停', tone: FolderStatusTone.paused);
  }
  switch (status.kind) {
    case BackupFolderStatusKindDto.uploading:
      return FolderStatusView(
        label: '正在备份 $done / $total',
        tone: FolderStatusTone.busy,
        progress: total <= 0 ? null : (done / total).clamp(0.0, 1.0).toDouble(),
        remaining: remaining,
      );
    case BackupFolderStatusKindDto.scanning:
      return const FolderStatusView(
        label: '正在检查…',
        tone: FolderStatusTone.busy,
      );
    case BackupFolderStatusKindDto.error:
      return FolderStatusView(
        label: status.unreadableCount > 0
            ? '${status.unreadableCount} 个文件无法读取'
            : '备份遇到问题',
        tone: FolderStatusTone.warn,
      );
    case BackupFolderStatusKindDto.waitingForWechatExit:
      return const FolderStatusView(
        label: '等微信退出后备份',
        tone: FolderStatusTone.paused,
      );
    case BackupFolderStatusKindDto.idleOk:
    case BackupFolderStatusKindDto.paused:
    case BackupFolderStatusKindDto.needsUnlock:
      if (status.unreadableCount > 0) {
        return FolderStatusView(
          label: '${status.unreadableCount} 个文件无法读取',
          tone: FolderStatusTone.warn,
        );
      }
      return const FolderStatusView(label: '已备份', tone: FolderStatusTone.ok);
  }
}
