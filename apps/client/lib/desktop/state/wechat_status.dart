import '../../core/rust/models.dart';

/// 微信页面状态胶囊的色调（对应 tokens.css `.pill-*`）。
enum WechatStatusTone { ok, busy, paused, warn, muted }

/// 一段展示文案 + 色调；纯函数生成，便于测试所有类别状态映射。
class WechatStatusView {
  const WechatStatusView({
    required this.label,
    required this.tone,
    this.detail,
    this.done,
    this.total,
  });

  final String label;
  final WechatStatusTone tone;

  /// 悬浮提示等附加说明（可选）。
  final String? detail;

  /// “正在备份 n / m”里的文件数。
  final int? done;
  final int? total;
}

String wechatVersionLabel(WechatVersionDto version) {
  return switch (version) {
    WechatVersionDto.wechat3 => '微信 3.x',
    WechatVersionDto.wechat4 => '微信 4.x',
  };
}

String wechatCategoryLabel(WechatCategoryDto category) {
  return switch (category) {
    WechatCategoryDto.chatFiles => '聊天文件',
    WechatCategoryDto.imagesAndVideos => '图片与视频',
    WechatCategoryDto.databases => '聊天记录数据库',
    WechatCategoryDto.phoneMigration => '手机迁移备份',
  };
}

/// 事件流里的微信来源标签（供“最近活动”/未来托盘活动复用）。
String wechatEventSourceLabel(WechatEventSourceDto source) {
  return '微信备份 · ${wechatCategoryLabel(source.category)}';
}

String wechatCategoryDescription(
  WechatCategoryDto category, {
  bool setup = false,
}) {
  if (setup) {
    return switch (category) {
      WechatCategoryDto.chatFiles => '收到和发送的文档、表格、安装包等',
      WechatCategoryDto.imagesAndVideos => '体积最大，可以以后再开启',
      WechatCategoryDto.databases => '微信退出时自动做快照',
      WechatCategoryDto.phoneMigration => '微信“迁移与备份”生成的数据',
    };
  }
  return switch (category) {
    WechatCategoryDto.chatFiles => '收到和发送的文档、表格、安装包等，可在网页上直接下载',
    WechatCategoryDto.imagesAndVideos => '原样备份（图片为微信自有格式，只能恢复到微信后查看）',
    WechatCategoryDto.databases => '整体快照，保留最近 3 份；只能恢复到微信',
    WechatCategoryDto.phoneMigration => '微信“迁移与备份”生成的数据',
  };
}

WechatCategoryInfoDto? wechatCategoryInfo(
  WechatAccountDto account,
  WechatCategoryDto category,
) {
  for (final info in account.categories) {
    if (info.category == category) {
      return info;
    }
  }
  return null;
}

WechatCategoryBackupStatusDto? wechatCategoryStatus(
  WechatAccountBackupStatusDto? status,
  WechatCategoryDto category,
) {
  if (status == null) {
    return null;
  }
  for (final item in status.categories) {
    if (item.category == category) {
      return item;
    }
  }
  return null;
}

WechatAccountRefDto wechatAccountRef(WechatAccountDto account) {
  return WechatAccountRefDto(
    dataRoot: account.dataRoot,
    accountName: account.accountName,
    version: account.version,
  );
}

/// 默认勾选（design D2）：聊天文件、图片与视频、聊天记录数据库；手机迁移
/// 备份只在账号确实存在该类数据时勾选。
Set<WechatCategoryDto> wechatDefaultCategories(
  Iterable<WechatAccountDto> accounts,
) {
  final selected = <WechatCategoryDto>{
    WechatCategoryDto.chatFiles,
    WechatCategoryDto.imagesAndVideos,
    WechatCategoryDto.databases,
  };
  for (final account in accounts) {
    final migration = wechatCategoryInfo(
      account,
      WechatCategoryDto.phoneMigration,
    );
    if (migration?.exists == true) {
      selected.add(WechatCategoryDto.phoneMigration);
      break;
    }
  }
  return selected;
}

/// 已保存类别里是否有任意一个勾选；没有就是“尚未开启”状态。
bool wechatAccountHasSelection(WechatAccountBackupStatusDto? status) {
  return status?.categories.any((item) => item.selected) ?? false;
}

/// 单个类别的状态文字。
///
/// 顺序固定：无内容 → 未选择 → 需要解锁 → 暂停 → 等微信退出 → 正在备份 →
/// 正在检查 → 错误 → 已备份。
WechatStatusView wechatCategoryStatusView({
  required WechatCategoryDto category,
  required WechatCategoryInfoDto? info,
  required WechatCategoryBackupStatusDto? status,
}) {
  if (info == null || !info.exists) {
    return const WechatStatusView(label: '无内容', tone: WechatStatusTone.muted);
  }
  if (status == null || !status.selected) {
    return const WechatStatusView(label: '未选择', tone: WechatStatusTone.muted);
  }
  return _categoryStatusView(status);
}

WechatStatusView _categoryStatusView(WechatCategoryBackupStatusDto status) {
  final folder = status.status;
  if (folder == null) {
    return const WechatStatusView(label: '等待备份', tone: WechatStatusTone.paused);
  }
  if (status.pauseReason == BackupPauseReasonDto.needsUnlock) {
    return const WechatStatusView(label: '需要解锁', tone: WechatStatusTone.warn);
  }
  switch (folder.kind) {
    case BackupFolderStatusKindDto.needsUnlock:
      return const WechatStatusView(label: '需要解锁', tone: WechatStatusTone.warn);
    case BackupFolderStatusKindDto.paused:
      return const WechatStatusView(label: '暂停', tone: WechatStatusTone.paused);
    case BackupFolderStatusKindDto.waitingForWechatExit:
      return const WechatStatusView(
        label: '等微信退出后备份',
        tone: WechatStatusTone.paused,
      );
    case BackupFolderStatusKindDto.uploading:
      return WechatStatusView(
        label: '正在备份 ${folder.done} / ${folder.total}',
        tone: WechatStatusTone.busy,
        done: folder.done,
        total: folder.total,
      );
    case BackupFolderStatusKindDto.scanning:
      return const WechatStatusView(
        label: '正在检查…',
        tone: WechatStatusTone.busy,
      );
    case BackupFolderStatusKindDto.error:
      return WechatStatusView(
        label: '错误',
        tone: WechatStatusTone.warn,
        detail: folder.unreadableCount > 0
            ? '${folder.unreadableCount} 个文件无法读取'
            : null,
      );
    case BackupFolderStatusKindDto.idleOk:
      return WechatStatusView(
        label: '已备份',
        tone: WechatStatusTone.ok,
        detail: folder.unreadableCount > 0
            ? '${folder.unreadableCount} 个文件无法读取'
            : null,
      );
  }
}

/// 账号卡片右上角的汇总胶囊。
///
/// 优先级：错误 > 正在上传 > 需要解锁 > 暂停 > 等微信退出 > 已备份。
/// “正在备份 n 个文件”与设计稿一致，用已完成文件数，而不是百分比。
WechatStatusView wechatAccountStatusView(
  WechatAccountBackupStatusDto? status, {
  bool globalPaused = false,
}) {
  final selected = status?.categories
      .where((item) => item.selected)
      .toList(growable: false);
  if (selected == null || selected.isEmpty) {
    return const WechatStatusView(label: '未开启', tone: WechatStatusTone.muted);
  }
  if (globalPaused) {
    return const WechatStatusView(label: '已暂停', tone: WechatStatusTone.paused);
  }
  var uploadingDone = 0;
  var hasUploading = false;
  var hasError = false;
  var errorFiles = 0;
  var hasUnlock = false;
  var hasPaused = false;
  var hasWaiting = false;
  var hasScanning = false;
  var waitingAll = true;
  for (final item in selected) {
    final view = _categoryStatusView(item);
    final kind = item.status?.kind;
    if (kind == BackupFolderStatusKindDto.uploading) {
      hasUploading = true;
      uploadingDone += item.status?.done ?? 0;
      waitingAll = false;
    } else if (kind == BackupFolderStatusKindDto.error) {
      hasError = true;
      errorFiles += item.status?.unreadableCount ?? 0;
      waitingAll = false;
    } else if (kind == BackupFolderStatusKindDto.needsUnlock ||
        item.pauseReason == BackupPauseReasonDto.needsUnlock) {
      hasUnlock = true;
      waitingAll = false;
    } else if (kind == BackupFolderStatusKindDto.paused) {
      hasPaused = true;
      waitingAll = false;
    } else if (kind == BackupFolderStatusKindDto.waitingForWechatExit) {
      hasWaiting = true;
    } else if (kind == BackupFolderStatusKindDto.scanning) {
      hasScanning = true;
      waitingAll = false;
    } else if (kind == null) {
      // 已勾选但还没有状态：仍等待第一次核对。
      hasWaiting = true;
      waitingAll = false;
    } else if (view.tone == WechatStatusTone.ok ||
        kind == BackupFolderStatusKindDto.idleOk) {
      waitingAll = false;
    }
  }
  if (hasError) {
    return WechatStatusView(
      label: errorFiles > 0 ? '$errorFiles 个文件无法读取' : '备份遇到问题',
      tone: WechatStatusTone.warn,
    );
  }
  if (hasUploading) {
    return WechatStatusView(
      label: '正在备份 $uploadingDone 个文件',
      tone: WechatStatusTone.busy,
    );
  }
  if (hasUnlock) {
    return const WechatStatusView(label: '需要解锁', tone: WechatStatusTone.warn);
  }
  if (hasPaused) {
    return const WechatStatusView(label: '已暂停', tone: WechatStatusTone.paused);
  }
  if (hasWaiting && waitingAll) {
    return const WechatStatusView(
      label: '等待微信退出',
      tone: WechatStatusTone.paused,
    );
  }
  if (hasWaiting) {
    return const WechatStatusView(
      label: '部分已备份 · 等待微信退出',
      tone: WechatStatusTone.paused,
    );
  }
  if (hasScanning) {
    return const WechatStatusView(label: '正在检查…', tone: WechatStatusTone.busy);
  }
  return const WechatStatusView(label: '已备份', tone: WechatStatusTone.ok);
}
