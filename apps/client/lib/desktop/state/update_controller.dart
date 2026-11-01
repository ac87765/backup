import 'dart:async';

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/clock.dart';
import '../../core/errors/bridge_error_text.dart';
import '../../core/providers/app_providers.dart';
import '../../core/rust/models.dart';
import '../lifecycle/app_quit.dart';

/// 更新界面的阶段（design/desktop/update.html 的各 data-when 状态）。
enum UpdatePhase {
  /// 尚未完成第一次检查。
  idle,

  /// 正在检查。
  checking,

  /// 开发构建：不检查更新，也不显示横幅 / 托盘项。
  development,

  /// 签名无效、网络失败等：不提示任何内容，只在“关于”里说明。
  unavailable,

  /// 已是最新版本。
  upToDate,

  /// 有新版本可下载。
  available,

  /// 正在下载 / 校验安装包。
  downloading,

  /// 下载或安装失败（文件已删除，不运行）。
  failed,

  /// 下载并通过校验，等待重新启动安装。
  completed,
}

/// 更新检查结果、横幅、下载与安装的统一状态。
class UpdateState {
  const UpdateState({
    this.check,
    this.dismissedVersion,
    this.checking = false,
    this.download,
    this.startingDownload = false,
    this.failure,
    this.installing = false,
    this.cardPostponed = false,
  });

  /// 最近一次检查结果；`null` 表示还没有完成过检查。
  final UpdateCheckDto? check;

  /// 用户关闭过横幅的版本；同版本不再显示横幅。
  final String? dismissedVersion;

  final bool checking;

  /// 最近一次下载进度事件（终态为 completed / failed / cancelled）。
  final UpdateProgressDto? download;

  final bool startingDownload;

  /// 下载 / 安装失败时面向用户的中文提示。
  final String? failure;

  final bool installing;

  /// 用户在“关于”里点了“稍后”，本次检查结果下不再显示更新卡片。
  final bool cardPostponed;

  UpdateInfoDto? get info {
    final result = check;
    if (result == null || result.state != UpdateCheckStateDto.available) {
      return null;
    }
    return result.info;
  }

  /// 当前版本低于清单的最低支持版本：登录前后都显示阻断页。
  bool get forced => info?.forced ?? false;

  /// 横幅是否显示：有新版本且这个版本还没被用户关闭。
  bool get bannerVisible {
    final available = info;
    return available != null && available.version != dismissedVersion;
  }

  UpdatePhase get phase {
    if (failure != null) {
      return UpdatePhase.failed;
    }
    final progress = download;
    if (progress != null) {
      switch (progress.status) {
        case UpdateDownloadStatusDto.downloading:
          return UpdatePhase.downloading;
        case UpdateDownloadStatusDto.completed:
          return UpdatePhase.completed;
        case UpdateDownloadStatusDto.failed:
          return UpdatePhase.failed;
        case UpdateDownloadStatusDto.cancelled:
          break;
      }
    }
    if (startingDownload) {
      return UpdatePhase.downloading;
    }
    if (checking) {
      return UpdatePhase.checking;
    }
    final result = check;
    if (result == null) {
      return UpdatePhase.idle;
    }
    return switch (result.state) {
      UpdateCheckStateDto.developmentBuild => UpdatePhase.development,
      UpdateCheckStateDto.unavailable => UpdatePhase.unavailable,
      UpdateCheckStateDto.upToDate => UpdatePhase.upToDate,
      UpdateCheckStateDto.available => UpdatePhase.available,
    };
  }

  UpdateState copyWith({
    UpdateCheckDto? check,
    String? dismissedVersion,
    bool? checking,
    UpdateProgressDto? download,
    bool clearDownload = false,
    bool? startingDownload,
    String? failure,
    bool clearFailure = false,
    bool? installing,
    bool? cardPostponed,
  }) {
    return UpdateState(
      check: check ?? this.check,
      dismissedVersion: dismissedVersion ?? this.dismissedVersion,
      checking: checking ?? this.checking,
      download: clearDownload ? null : (download ?? this.download),
      startingDownload: startingDownload ?? this.startingDownload,
      failure: clearFailure ? null : (failure ?? this.failure),
      installing: installing ?? this.installing,
      cardPostponed: cardPostponed ?? this.cardPostponed,
    );
  }
}

/// 检查 / 下载定时器；测试可用 Flutter 的 FakeAsync（`tester.pump`）推进。
abstract class UpdateScheduler {
  void schedule(Duration delay, void Function() callback);

  void cancel();
}

class TimerUpdateScheduler implements UpdateScheduler {
  Timer? _timer;

  @override
  void schedule(Duration delay, void Function() callback) {
    _timer?.cancel();
    _timer = Timer(delay, callback);
  }

  @override
  void cancel() {
    _timer?.cancel();
    _timer = null;
  }
}

final updateSchedulerProvider = Provider<UpdateScheduler>((ref) {
  final scheduler = TimerUpdateScheduler();
  ref.onDispose(scheduler.cancel);
  return scheduler;
});

/// 安装动作的结果：成功启动安装、需要手动拖动 .dmg、或失败（错误在 state）。
enum UpdateInstallResult { started, manualInstallRequired, failed }

/// 更新检查调度：启动后 10 秒首次检查，之后每 24 小时检查一次；手动
/// “检查更新”立即执行并重置 24 小时计时。
class UpdateController extends Notifier<UpdateState> {
  static const Duration initialDelay = Duration(seconds: 10);
  static const Duration interval = Duration(hours: 24);

  late final UpdateScheduler _scheduler;
  StreamSubscription<UpdateProgressDto>? _progress;
  bool _forcedStopRequested = false;

  @override
  UpdateState build() {
    _scheduler = ref.read(updateSchedulerProvider);
    unawaited(_start());
    _scheduler.schedule(initialDelay, () => unawaited(checkNow()));
    return const UpdateState();
  }

  /// 等 Rust 初始化完成后再订阅进度流 / 读取已关闭横幅的版本，避免在
  /// `RustLib.init` 之前调用生成函数。
  Future<void> _start() async {
    try {
      await ref.read(bootstrapProvider.future);
    } catch (_) {
      // 初始化失败时仍允许界面运行；检查会返回 unavailable。
    }
    if (!ref.mounted) {
      return;
    }
    try {
      _progress = ref
          .read(nativeBridgeProvider)
          .updateProgressStream()
          .listen(_onProgress);
      ref.onDispose(() {
        unawaited(_progress?.cancel());
      });
    } catch (_) {
      // 订阅失败时界面仍可手动检查 / 下载，进度轮询由后续版本补齐。
    }
    await _loadDismissedVersion();
  }

  Future<void> _loadDismissedVersion() async {
    try {
      final version = await ref
          .read(nativeBridgeProvider)
          .updateBannerDismissedVersion();
      if (!ref.mounted || version == null || state.dismissedVersion != null) {
        return;
      }
      state = state.copyWith(dismissedVersion: version);
    } catch (_) {
      // 读不到时按“没有关闭过横幅”处理；下次关闭会写入。
    }
  }

  /// 立即检查更新。自动检查与“关于”页手动按钮都走这里。
  Future<void> checkNow() async {
    if (!ref.mounted ||
        state.checking ||
        state.phase == UpdatePhase.downloading ||
        state.installing) {
      return;
    }
    state = state.copyWith(
      checking: true,
      clearFailure: true,
      clearDownload: true,
      cardPostponed: false,
    );
    try {
      // 检查与账号无关；等初始化完成即可，不等待登录。
      try {
        await ref.read(bootstrapProvider.future);
      } catch (_) {
        // 初始化失败也继续检查；桥接会返回 unavailable。
      }
      if (!ref.mounted) {
        return;
      }
      final now = ref.read(clockProvider)().millisecondsSinceEpoch ~/ 1000;
      final result = await ref.read(nativeBridgeProvider).updateCheck(now);
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(check: result, checking: false);
      _handleForced(result);
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(
        checking: false,
        check: UpdateCheckDto(
          state: UpdateCheckStateDto.unavailable,
          checkedAt: ref.read(clockProvider)().millisecondsSinceEpoch ~/ 1000,
          reason: 'exception',
        ),
      );
    } finally {
      if (ref.mounted) {
        _scheduler.schedule(interval, () => unawaited(checkNow()));
      }
    }
  }

  void _handleForced(UpdateCheckDto result) {
    final isForced =
        result.state == UpdateCheckStateDto.available &&
        (result.info?.forced ?? false);
    if (!isForced) {
      _forcedStopRequested = false;
      return;
    }
    if (_forcedStopRequested) {
      return;
    }
    _forcedStopRequested = true;
    // 强制更新页不启动备份；若引擎已在后台运行则停掉（不删除本地数据）。
    unawaited(_stopBackupEngine());
  }

  Future<void> _stopBackupEngine() async {
    try {
      await ref.read(nativeBridgeProvider).backupStop();
    } catch (_) {
      // 登录前引擎本来就没启动；失败也不影响阻断页。
    }
  }

  /// 关闭横幅并记录版本；同一版本以后不再出现（“关于”里仍可看到更新）。
  Future<void> dismissBanner() async {
    final available = state.info;
    if (available == null) {
      return;
    }
    state = state.copyWith(dismissedVersion: available.version);
    try {
      await ref
          .read(nativeBridgeProvider)
          .updateDismissBanner(available.version);
    } catch (_) {
      // 本地已隐藏；写入失败只影响下次启动。
    }
  }

  /// 用户点“稍后”：隐藏本次检查的更新卡片，下一次检查会重新出现。
  void postponeCard() {
    state = state.copyWith(cardPostponed: true);
  }

  /// 开始下载最近一次检查到的新版本；进度由桥接事件流推送。
  Future<void> startDownload() async {
    if (!ref.mounted) {
      return;
    }
    final available = state.info;
    if (available == null ||
        state.phase == UpdatePhase.downloading ||
        state.startingDownload ||
        state.installing) {
      return;
    }
    state = state.copyWith(
      startingDownload: true,
      clearFailure: true,
      clearDownload: true,
      cardPostponed: false,
    );
    try {
      final downloadId = await ref.read(nativeBridgeProvider).updateDownload();
      if (!ref.mounted) {
        return;
      }
      if (state.download == null) {
        state = state.copyWith(
          startingDownload: false,
          download: UpdateProgressDto(
            downloadId: downloadId,
            status: UpdateDownloadStatusDto.downloading,
            downloadedBytes: 0,
            totalBytes: available.size,
            speedBytesPerSecond: 0,
          ),
        );
      } else {
        // 进度事件可能先于 updateDownload 返回：保留更新的事件。
        state = state.copyWith(startingDownload: false);
      }
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(
        startingDownload: false,
        failure: bridgeErrorText(error),
      );
    }
  }

  /// 取消当前下载；没有下载时界面不应调用。
  Future<void> cancelDownload() async {
    final current = state.download;
    if (current == null ||
        current.status != UpdateDownloadStatusDto.downloading) {
      return;
    }
    try {
      await ref.read(nativeBridgeProvider).updateCancelDownload();
    } catch (_) {
      // 取消失败时保持进度，由下一次进度事件纠正。
      return;
    }
    if (!ref.mounted) {
      return;
    }
    final latest = state.download;
    if (latest != null &&
        latest.downloadId == current.downloadId &&
        latest.status == UpdateDownloadStatusDto.downloading) {
      state = state.copyWith(clearDownload: true);
    }
  }

  /// 安装已校验的更新。Windows / macOS 的 `started` 都走完整退出流程。
  Future<UpdateInstallResult> install() async {
    if (!ref.mounted || state.installing) {
      return UpdateInstallResult.failed;
    }
    state = state.copyWith(installing: true, clearFailure: true);
    try {
      await ref.read(nativeBridgeProvider).updateInstall();
      try {
        await ref.read(appQuitProvider).quit();
      } catch (error) {
        if (ref.mounted) {
          state = state.copyWith(
            installing: false,
            failure: bridgeErrorText(error),
          );
        }
        return UpdateInstallResult.failed;
      }
      return UpdateInstallResult.started;
    } catch (error) {
      if (!ref.mounted) {
        return UpdateInstallResult.failed;
      }
      if (error is BridgeError && error.code == 'manual_install_required') {
        try {
          await ref.read(nativeBridgeProvider).updateOpenDmg();
        } catch (_) {
          // 打不开时仍提示用户手动拖动；关于页有“下载页面”入口。
        }
        if (ref.mounted) {
          state = state.copyWith(installing: false);
        }
        return UpdateInstallResult.manualInstallRequired;
      }
      state = state.copyWith(
        installing: false,
        failure: bridgeErrorText(error),
      );
      return UpdateInstallResult.failed;
    }
  }

  void _onProgress(UpdateProgressDto event) {
    if (!ref.mounted) {
      return;
    }
    final current = state.download;
    if (event.status == UpdateDownloadStatusDto.cancelled) {
      if (current == null || current.downloadId == event.downloadId) {
        state = state.copyWith(clearDownload: true);
      }
      return;
    }
    if (current != null &&
        current.downloadId != event.downloadId &&
        current.status == UpdateDownloadStatusDto.downloading) {
      // 忽略上一次下载的迟到事件。
      return;
    }
    state = state.copyWith(download: event, startingDownload: false);
  }
}

final updateControllerProvider =
    NotifierProvider<UpdateController, UpdateState>(UpdateController.new);
