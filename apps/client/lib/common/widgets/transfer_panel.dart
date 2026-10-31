import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/rust/models.dart';
import '../../core/theme/design_tokens.dart';
import '../state/transfer_controller.dart';
import 'controls.dart';
import 'file_icon.dart';

/// 状态栏“传输”打开的传输面板（files.html 的 transfers 状态）。
class TransferPanel extends ConsumerWidget {
  const TransferPanel({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = DesktopTokens.of(context);
    final state = ref.watch(transferControllerProvider);
    final controller = ref.read(transferControllerProvider.notifier);
    final active = state.activeCount;
    return Container(
      decoration: BoxDecoration(
        color: tokens.surface,
        border: Border.all(color: tokens.line),
        borderRadius: BorderRadius.circular(tokens.radiusCard),
        boxShadow: tokens.shadowPop,
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
            child: Row(
              children: [
                Text(
                  '传输',
                  style: TextStyle(
                    color: tokens.ink900,
                    fontSize: tokens.textMd,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                const SizedBox(width: 8),
                Text(
                  active > 0 ? '$active 项进行中' : '已全部结束',
                  style: TextStyle(
                    color: tokens.ink500,
                    fontSize: tokens.textXs,
                  ),
                ),
                const Spacer(),
                if (active > 0)
                  TokensIconButton(
                    icon: LucideIcons.pause,
                    tooltip: '全部暂停',
                    onPressed: controller.pauseAll,
                  ),
                TokensIconButton(
                  icon: LucideIcons.x,
                  tooltip: '关闭传输面板',
                  onPressed: () =>
                      ref.read(transferPanelOpenProvider.notifier).close(),
                ),
              ],
            ),
          ),
          Container(height: 1, color: tokens.line),
          if (state.items.isEmpty)
            Padding(
              padding: const EdgeInsets.all(20),
              child: Text(
                '没有进行中的传输。上传或下载文件后会显示在这里。',
                style: TextStyle(color: tokens.ink500, fontSize: tokens.textXs),
              ),
            )
          else
            ConstrainedBox(
              constraints: const BoxConstraints(maxHeight: 430),
              child: SingleChildScrollView(
                child: Column(
                  children: [
                    for (var index = 0; index < state.items.length; index += 1)
                      _TransferRow(
                        item: state.items[index],
                        last: index == state.items.length - 1,
                      ),
                  ],
                ),
              ),
            ),
        ],
      ),
    );
  }
}

class _TransferRow extends ConsumerWidget {
  const _TransferRow({required this.item, required this.last});

  final TransferItem item;
  final bool last;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = DesktopTokens.of(context);
    final controller = ref.read(transferControllerProvider.notifier);
    final warning = item.status == TransferStatusDto.retrying;
    final metaColor = warning ? tokens.warning600 : tokens.ink500;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
      decoration: BoxDecoration(
        border: last ? null : Border(bottom: BorderSide(color: tokens.line)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Row(
            children: [
              FileIconTile(name: item.name, folder: item.isFolder),
              const SizedBox(width: 12),
              Expanded(
                child: Text(
                  item.name,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    color: tokens.ink900,
                    fontSize: tokens.textSm,
                    fontWeight: FontWeight.w500,
                  ),
                ),
              ),
              ..._actions(controller),
            ],
          ),
          const SizedBox(height: 8),
          _ProgressBar(
            percent: item.percent,
            paused: item.isPaused,
            failed: item.status == TransferStatusDto.failed,
          ),
          const SizedBox(height: 6),
          Row(
            children: [
              Expanded(
                child: Text(
                  _leftMeta(item),
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(color: metaColor, fontSize: tokens.textXs),
                ),
              ),
              if (item.speedText != null) ...[
                const SizedBox(width: 8),
                Text(
                  item.speedText!,
                  style: TextStyle(
                    color: tokens.ink500,
                    fontSize: tokens.textXs,
                  ),
                ),
              ] else if (warning || item.pendingSessionId != null) ...[
                const SizedBox(width: 8),
                Text(
                  item.progressDetail,
                  style: TextStyle(
                    color: tokens.ink500,
                    fontSize: tokens.textXs,
                  ),
                ),
              ],
            ],
          ),
          if (item.errorMessage != null && item.pendingSessionId == null) ...[
            const SizedBox(height: 4),
            Text(
              '${item.isUpload ? '上传' : '下载'}失败：${item.errorMessage}',
              style: TextStyle(
                color: tokens.danger700,
                fontSize: tokens.textXs,
              ),
            ),
          ],
          if (item.pendingSessionId != null) ...[
            const SizedBox(height: 6),
            Row(
              mainAxisAlignment: MainAxisAlignment.end,
              children: [
                TokensButton(
                  key: ValueKey<String>('pending-resume-${item.id}'),
                  label: '续传',
                  icon: LucideIcons.play,
                  onPressed: () =>
                      controller.resumePending(item.pendingSessionId!, item.id),
                ),
                const SizedBox(width: 8),
                TokensButton(
                  key: ValueKey<String>('pending-abandon-${item.id}'),
                  label: '放弃',
                  variant: TokensButtonVariant.ghost,
                  onPressed: () => controller.abandonPending(
                    item.pendingSessionId!,
                    item.id,
                  ),
                ),
              ],
            ),
          ],
        ],
      ),
    );
  }

  List<Widget> _actions(TransferController controller) {
    if (item.pendingSessionId != null) {
      return const <Widget>[];
    }
    return <Widget>[
      if (item.canPause)
        TokensIconButton(
          key: ValueKey<String>('transfer-pause-${item.id}'),
          icon: LucideIcons.pause,
          tooltip: '暂停',
          onPressed: () => controller.pause(item.id),
        )
      else if (item.canResume)
        TokensIconButton(
          key: ValueKey<String>('transfer-resume-${item.id}'),
          icon: LucideIcons.play,
          tooltip: '继续',
          onPressed: () => controller.resume(item.id),
        )
      else if (item.status == TransferStatusDto.failed)
        TokensIconButton(
          key: ValueKey<String>('transfer-retry-${item.id}'),
          icon: LucideIcons.refreshCw,
          tooltip: '重试',
          onPressed: () => controller.retry(item.id),
        )
      else if (item.isTerminal)
        TokensIconButton(
          key: ValueKey<String>('transfer-dismiss-${item.id}'),
          icon: LucideIcons.x,
          tooltip: '清除',
          onPressed: () => controller.dismiss(item.id),
        ),
      if (item.canCancel)
        TokensIconButton(
          key: ValueKey<String>('transfer-cancel-${item.id}'),
          icon: LucideIcons.x,
          tooltip: '取消',
          onPressed: () => controller.cancel(item.id),
        ),
    ];
  }

  String _leftMeta(TransferItem item) {
    if (item.pendingSessionId != null) {
      return '待续传';
    }
    if (item.status == TransferStatusDto.retrying) {
      return '服务器繁忙，稍后自动继续';
    }
    final direction = item.isFolder
        ? (item.isUpload ? '备份' : '下载')
        : (item.isUpload ? '上传' : '下载');
    final detail = item.progressDetail;
    if (detail.isEmpty) {
      return '$direction · ${item.statusLabel}';
    }
    return '$direction · ${item.statusLabel} · $detail';
  }
}

class _ProgressBar extends StatelessWidget {
  const _ProgressBar({
    required this.percent,
    required this.paused,
    required this.failed,
  });

  final int percent;
  final bool paused;
  final bool failed;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final color = failed
        ? tokens.danger600
        : paused
        ? tokens.ink400
        : tokens.brand600;
    return Container(
      height: 4,
      decoration: BoxDecoration(
        color: tokens.canvasStrong,
        borderRadius: BorderRadius.circular(2),
      ),
      clipBehavior: Clip.antiAlias,
      child: Align(
        alignment: Alignment.centerLeft,
        child: FractionallySizedBox(
          widthFactor: percent / 100,
          child: Container(color: color),
        ),
      ),
    );
  }
}
