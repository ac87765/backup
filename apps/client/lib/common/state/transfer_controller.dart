import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/clock.dart';
import '../../core/errors/bridge_error_text.dart';
import '../../core/format/format.dart';
import '../../core/providers/app_providers.dart';
import '../../core/rust/models.dart';
import 'auth_controller.dart';

enum TransferJobKind {
  upload,
  download,
  uploadFolder,
  downloadFolder,
  wechatRestore,
}

@immutable
class TransferItem {
  const TransferItem({
    required this.id,
    required this.name,
    required this.kind,
    this.status = TransferStatusDto.pending,
    this.completedBytes = 0,
    this.totalBytes = 0,
    this.completedChunks = 0,
    this.totalChunks = 0,
    this.doneFiles = 0,
    this.totalFiles = 0,
    this.errorCode,
    this.errorMessage,
    this.speedBytesPerSecond,
    this.eta,
    this.pendingSessionId,
  });

  final String id;
  final String name;
  final TransferJobKind kind;
  final TransferStatusDto status;
  final int completedBytes;
  final int totalBytes;
  final int completedChunks;
  final int totalChunks;
  final int doneFiles;
  final int totalFiles;
  final String? errorCode;
  final String? errorMessage;
  final double? speedBytesPerSecond;
  final Duration? eta;

  /// 非空表示这是本机状态库里的未完成上传（应用重启后待续传）。
  final String? pendingSessionId;

  bool get isUpload =>
      kind == TransferJobKind.upload || kind == TransferJobKind.uploadFolder;

  bool get isFolder =>
      kind == TransferJobKind.uploadFolder ||
      kind == TransferJobKind.downloadFolder;

  String get directionLabel {
    return switch (kind) {
      TransferJobKind.upload => '上传',
      TransferJobKind.download => '下载',
      TransferJobKind.uploadFolder => '备份',
      TransferJobKind.downloadFolder => '下载',
      TransferJobKind.wechatRestore => '恢复',
    };
  }

  bool get isActive =>
      status == TransferStatusDto.pending ||
      status == TransferStatusDto.uploading ||
      status == TransferStatusDto.downloading ||
      status == TransferStatusDto.retrying;

  bool get isPaused => status == TransferStatusDto.paused;

  bool get isTerminal =>
      status == TransferStatusDto.completed ||
      status == TransferStatusDto.failed ||
      status == TransferStatusDto.cancelled;

  bool get canPause => isActive && pendingSessionId == null;

  bool get canResume => isPaused && pendingSessionId == null;

  bool get canCancel => (isActive || isPaused) && pendingSessionId == null;

  int get percent {
    if (totalBytes > 0) {
      return ((completedBytes / totalBytes) * 100).clamp(0, 100).round();
    }
    if (totalChunks > 0) {
      return ((completedChunks / totalChunks) * 100).clamp(0, 100).round();
    }
    return status == TransferStatusDto.completed ? 100 : 0;
  }

  String get statusLabel {
    if (pendingSessionId != null) {
      return '待续传';
    }
    switch (status) {
      case TransferStatusDto.pending:
        return '等待中';
      case TransferStatusDto.uploading:
        return '上传中';
      case TransferStatusDto.downloading:
        return '下载中';
      case TransferStatusDto.paused:
        return '已暂停';
      case TransferStatusDto.retrying:
        return '服务器繁忙，稍后自动继续';
      case TransferStatusDto.completed:
        return '已完成';
      case TransferStatusDto.failed:
        return '失败';
      case TransferStatusDto.cancelled:
        return '已取消';
    }
  }

  String get progressDetail {
    if (isFolder) {
      return '$doneFiles / $totalFiles 个文件';
    }
    if (totalBytes > 0) {
      return '${formatBytes(completedBytes)} / ${formatBytes(totalBytes)}';
    }
    if (totalChunks > 0) {
      return '$completedChunks / $totalChunks 块';
    }
    return '';
  }

  String? get speedText {
    final speed = speedBytesPerSecond;
    if (speed == null || speed <= 0) {
      return null;
    }
    final etaText = formatEta(eta);
    final formatted = formatSpeed(speed);
    return etaText.isEmpty ? formatted : '$formatted · $etaText';
  }

  TransferItem copyWith({
    String? id,
    String? name,
    TransferJobKind? kind,
    TransferStatusDto? status,
    int? completedBytes,
    int? totalBytes,
    int? completedChunks,
    int? totalChunks,
    int? doneFiles,
    int? totalFiles,
    String? errorCode,
    String? errorMessage,
    bool clearError = false,
    double? speedBytesPerSecond,
    Duration? eta,
    String? pendingSessionId,
    bool clearPendingSession = false,
  }) {
    return TransferItem(
      id: id ?? this.id,
      name: name ?? this.name,
      kind: kind ?? this.kind,
      status: status ?? this.status,
      completedBytes: completedBytes ?? this.completedBytes,
      totalBytes: totalBytes ?? this.totalBytes,
      completedChunks: completedChunks ?? this.completedChunks,
      totalChunks: totalChunks ?? this.totalChunks,
      doneFiles: doneFiles ?? this.doneFiles,
      totalFiles: totalFiles ?? this.totalFiles,
      errorCode: clearError ? null : (errorCode ?? this.errorCode),
      errorMessage: clearError ? null : (errorMessage ?? this.errorMessage),
      speedBytesPerSecond: speedBytesPerSecond ?? this.speedBytesPerSecond,
      eta: eta ?? this.eta,
      pendingSessionId: clearPendingSession
          ? null
          : (pendingSessionId ?? this.pendingSessionId),
    );
  }
}

@immutable
class TransferState {
  const TransferState({this.items = const <TransferItem>[]});

  final List<TransferItem> items;

  int get activeCount => items.where((item) => item.isActive).length;

  bool get hasItems => items.isNotEmpty;

  bool get allTerminal =>
      items.isNotEmpty && items.every((item) => item.isTerminal);

  int get totalCount => items.length;

  TransferState copyWith({List<TransferItem>? items}) {
    return TransferState(items: items ?? this.items);
  }
}

class _SpeedSample {
  const _SpeedSample(this.bytes, this.at);

  final int bytes;
  final DateTime at;
}

/// 传输列表状态：进度、速度/剩余时间、暂停 / 继续 / 取消 / 重试、
/// 429 退避提示，以及应用重启后的待续传上传。
class TransferController extends Notifier<TransferState> {
  final Map<String, _SpeedSample> _samples = <String, _SpeedSample>{};
  final Map<String, String> _knownNames = <String, String>{};
  bool _loadingPending = false;

  @override
  TransferState build() {
    // 直接订阅桥接事件流，而不是经过 StreamProvider：StreamProvider 只保留最新值，
    // 同一帧内的多个进度事件（例如暂停后立即进入限流重试、完成事件）会被合并丢失。
    final bridge = ref.watch(nativeBridgeProvider);
    final transferSub = bridge.transferProgressStream().listen(_applyProgress);
    final folderSub = bridge.folderProgressStream().listen(
      _applyFolderProgress,
    );
    ref.onDispose(() {
      transferSub.cancel();
      folderSub.cancel();
    });
    ref.listen(authControllerProvider, (previous, next) {
      if (next.stage == AuthStage.ready && previous?.stage != AuthStage.ready) {
        Future<void>.microtask(_loadPendingUploads);
      }
    });
    if (ref.read(authControllerProvider).stage == AuthStage.ready) {
      Future<void>.microtask(_loadPendingUploads);
    }
    return const TransferState();
  }

  bool get canWrite {
    final account = ref.read(authControllerProvider).account;
    return account != null && account.writesAllowed && account.unlocked;
  }

  Future<void> startUploadFiles(String parentId, List<String> paths) async {
    _requireWritable();
    final bridge = ref.read(nativeBridgeProvider);
    for (final path in paths) {
      final id = await bridge.startUploadFile(parentId, path);
      _knownNames[id] = baseName(path);
      _upsert(
        TransferItem(
          id: id,
          name: baseName(path),
          kind: TransferJobKind.upload,
        ),
      );
    }
  }

  Future<void> startUploadFolder(String parentId, String localDir) async {
    _requireWritable();
    final id = await ref
        .read(nativeBridgeProvider)
        .startUploadFolder(parentId, localDir);
    final name = baseName(localDir);
    _knownNames[id] = name;
    _upsert(
      TransferItem(id: id, name: name, kind: TransferJobKind.uploadFolder),
    );
  }

  Future<void> startUploadNewVersion(String nodeId, String path) async {
    _requireWritable();
    final id = await ref
        .read(nativeBridgeProvider)
        .startUploadNewVersion(nodeId, path);
    _knownNames[id] = baseName(path);
    _upsert(
      TransferItem(id: id, name: baseName(path), kind: TransferJobKind.upload),
    );
  }

  Future<void> startDownload({
    required String nodeId,
    required String versionId,
    required String fileName,
    required String destinationDir,
  }) async {
    final id = await ref
        .read(nativeBridgeProvider)
        .startDownloadFile(
          nodeId: nodeId,
          versionId: versionId,
          fileName: fileName,
          destinationDir: destinationDir,
          policy: ConflictPolicyDto.keepBoth,
        );
    _knownNames[id] = fileName;
    _upsert(
      TransferItem(id: id, name: fileName, kind: TransferJobKind.download),
    );
  }

  Future<void> startDownloadFolder({
    required String folderId,
    required String folderName,
    required String destinationRoot,
    ConflictPolicyDto policy = ConflictPolicyDto.keepBoth,
  }) async {
    final id = await ref
        .read(nativeBridgeProvider)
        .startDownloadFolder(
          folderId: folderId,
          folderName: folderName,
          destinationRoot: destinationRoot,
          policy: policy,
        );
    _knownNames[id] = folderName;
    _upsert(
      TransferItem(
        id: id,
        name: folderName,
        kind: TransferJobKind.downloadFolder,
      ),
    );
  }

  // 乐观更新只在等待桥接返回期间没有收到更新的进度事件时生效：
  // 进度流里更新的状态（例如限流重试、已完成）不能被迟到的乐观值覆盖。
  /// 开始一次微信恢复；进度通过共享的 `transferProgressStream` 推送。
  ///
  /// 恢复任务在桥接层创建前已检查微信进程与快照完整性，因此这里只负责把
  /// 任务加入传输列表，失败时把错误原样抛给对话框。
  Future<String> startWechatRestore(WechatRestoreRequestDto request) async {
    final id = await ref.read(nativeBridgeProvider).wechatStartRestore(request);
    final name = '恢复到微信 · ${request.accountName}';
    _knownNames[id] = name;
    _upsert(
      TransferItem(id: id, name: name, kind: TransferJobKind.wechatRestore),
    );
    return id;
  }

  Future<void> pause(String id) async {
    final before = _find(id)?.status;
    await ref.read(nativeBridgeProvider).pauseTransfer(id);
    _patch(
      id,
      (item) => item.status == before
          ? item.copyWith(status: TransferStatusDto.paused)
          : item,
    );
  }

  Future<void> resume(String id) async {
    final before = _find(id)?.status;
    await ref.read(nativeBridgeProvider).resumeTransfer(id);
    _patch(
      id,
      (item) => item.status == before
          ? item.copyWith(status: TransferStatusDto.pending)
          : item,
    );
  }

  Future<void> cancel(String id) async {
    await ref.read(nativeBridgeProvider).cancelTransfer(id);
    _samples.remove(id);
    _patch(id, (item) => item.copyWith(status: TransferStatusDto.cancelled));
  }

  Future<void> retry(String id) async {
    await ref.read(nativeBridgeProvider).retryTransfer(id);
    _patch(
      id,
      (item) =>
          item.copyWith(status: TransferStatusDto.retrying, clearError: true),
    );
  }

  Future<void> pauseAll() async {
    for (final item in state.items.where((item) => item.canPause)) {
      try {
        await pause(item.id);
      } catch (_) {
        // 单项失败不阻塞其他暂停请求。
      }
    }
  }

  Future<void> resumePending(String pendingSessionId, String itemId) async {
    final id = await ref
        .read(nativeBridgeProvider)
        .resumePendingUpload(pendingSessionId);
    final existing = _find(itemId);
    _remove(itemId);
    _knownNames[id] = existing?.name ?? '上传';
    _upsert(
      TransferItem(
        id: id,
        name: existing?.name ?? '上传',
        kind: TransferJobKind.upload,
      ),
    );
  }

  Future<void> abandonPending(String pendingSessionId, String itemId) async {
    await ref.read(nativeBridgeProvider).abortPendingUpload(pendingSessionId);
    _remove(itemId);
  }

  void clearCompleted() {
    state = state.copyWith(
      items: state.items
          .where((item) => !item.isTerminal)
          .toList(growable: false),
    );
  }

  void dismiss(String id) {
    _remove(id);
  }

  Future<void> _loadPendingUploads() async {
    if (_loadingPending) {
      return;
    }
    _loadingPending = true;
    try {
      final pending = await ref.read(nativeBridgeProvider).listPendingUploads();
      if (!ref.mounted) {
        return;
      }
      final existingSessions = <String>{
        for (final item in state.items)
          if (item.pendingSessionId != null) item.pendingSessionId!,
      };
      final additions = <TransferItem>[];
      for (final record in pending) {
        if (existingSessions.contains(record.sessionId)) {
          continue;
        }
        final status = switch (record.status) {
          PendingUploadStatusDto.uploading => TransferStatusDto.paused,
          PendingUploadStatusDto.paused => TransferStatusDto.paused,
          PendingUploadStatusDto.failed => TransferStatusDto.failed,
        };
        additions.add(
          TransferItem(
            id: 'pending:${record.sessionId}',
            name: baseName(record.localPath),
            kind: TransferJobKind.upload,
            status: status,
            totalBytes: record.declaredSize,
            completedBytes: (record.chunkCount * 4 * 1024 * 1024)
                .clamp(0, record.declaredSize)
                .toInt(),
            pendingSessionId: record.sessionId,
            errorMessage: status == TransferStatusDto.failed
                ? '上次上传未完成，可续传或放弃'
                : null,
          ),
        );
      }
      if (additions.isEmpty) {
        return;
      }
      state = state.copyWith(items: [...state.items, ...additions]);
    } catch (_) {
      // 本机状态库不可用时不影响新传输。
    } finally {
      _loadingPending = false;
    }
  }

  void _applyProgress(TransferProgressDto event) {
    final existing = _find(event.transferId);
    final now = ref.read(clockProvider)();
    final speed = _sampleSpeed(event.transferId, event.completedBytes, now);
    final item =
        (existing ??
                TransferItem(
                  id: event.transferId,
                  name: _knownNames[event.transferId] ?? '传输任务',
                  kind: event.direction == TransferDirectionDto.upload
                      ? TransferJobKind.upload
                      : TransferJobKind.download,
                ))
            .copyWith(
              status: event.status,
              completedBytes: event.completedBytes,
              totalBytes: event.totalBytes,
              completedChunks: event.completedChunks,
              totalChunks: event.totalChunks,
              errorCode: event.error?.code,
              errorMessage: event.error?.message,
              clearError: event.error == null,
              speedBytesPerSecond: speed,
              eta: _eta(event.completedBytes, event.totalBytes, speed),
            );
    if (item.isTerminal) {
      _samples.remove(item.id);
    }
    _upsert(item);
  }

  void _applyFolderProgress(FolderProgressDto event) {
    final existing = _find(event.jobId);
    final now = ref.read(clockProvider)();
    final speed = _sampleSpeed(event.jobId, event.completedBytes, now);
    final runningStatus = existing?.kind == TransferJobKind.uploadFolder
        ? TransferStatusDto.uploading
        : TransferStatusDto.downloading;
    final status = switch (event.status) {
      FolderJobStatusDto.scanning ||
      FolderJobStatusDto.planning ||
      FolderJobStatusDto.running =>
        existing?.status == TransferStatusDto.paused
            ? TransferStatusDto.paused
            : runningStatus,
      FolderJobStatusDto.done => TransferStatusDto.completed,
      FolderJobStatusDto.cancelled => TransferStatusDto.cancelled,
      FolderJobStatusDto.failed => TransferStatusDto.failed,
    };
    final item =
        (existing ??
                TransferItem(
                  id: event.jobId,
                  name: _knownNames[event.jobId] ?? '文件夹传输',
                  kind: TransferJobKind.downloadFolder,
                ))
            .copyWith(
              status: status,
              completedBytes: event.completedBytes,
              totalBytes: event.totalBytes,
              doneFiles: event.completedFiles,
              totalFiles: event.totalFiles,
              errorCode: event.error?.code,
              errorMessage: event.error?.message,
              clearError: event.error == null,
              speedBytesPerSecond: speed,
              eta: _eta(event.completedBytes, event.totalBytes, speed),
            );
    if (item.isTerminal) {
      _samples.remove(item.id);
    }
    _upsert(item);
  }

  double? _sampleSpeed(String id, int bytes, DateTime now) {
    final previous = _samples[id];
    _samples[id] = _SpeedSample(bytes, now);
    if (previous == null) {
      return null;
    }
    final elapsed = now.difference(previous.at).inMilliseconds / 1000;
    final delta = bytes - previous.bytes;
    if (elapsed <= 0 || delta <= 0) {
      return null;
    }
    return delta / elapsed;
  }

  Duration? _eta(int completed, int total, double? speed) {
    if (speed == null || speed <= 0 || total <= completed) {
      return null;
    }
    final seconds = ((total - completed) / speed).ceil();
    return Duration(seconds: seconds);
  }

  TransferItem? _find(String id) {
    for (final item in state.items) {
      if (item.id == id) {
        return item;
      }
    }
    return null;
  }

  void _upsert(TransferItem next) {
    final items = <TransferItem>[...state.items];
    final index = items.indexWhere((item) => item.id == next.id);
    if (index < 0) {
      items.add(next);
    } else {
      items[index] = next;
    }
    state = state.copyWith(items: items);
  }

  void _patch(String id, TransferItem Function(TransferItem item) update) {
    final existing = _find(id);
    if (existing != null) {
      _upsert(update(existing));
    }
  }

  void _remove(String id) {
    state = state.copyWith(
      items: state.items.where((item) => item.id != id).toList(growable: false),
    );
  }

  void _requireWritable() {
    if (!canWrite) {
      throw const BridgeError(
        code: 'account_not_writable',
        message: '账号通过审核后才能上传和备份文件',
      );
    }
  }
}

final transferControllerProvider =
    NotifierProvider<TransferController, TransferState>(TransferController.new);

/// 传输面板是否展开（状态栏右侧“传输”打开）。
class TransferPanelController extends Notifier<bool> {
  @override
  bool build() => false;

  void open() => state = true;

  void close() => state = false;

  void toggle() => state = !state;
}

final transferPanelOpenProvider =
    NotifierProvider<TransferPanelController, bool>(
      TransferPanelController.new,
    );

/// 供界面展示的错误文案（避免直接依赖具体异常类型）。
String transferErrorText(Object error) => bridgeErrorText(error);
