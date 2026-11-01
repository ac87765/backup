import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/clock.dart';
import '../../core/format/format.dart';
import '../../core/rust/models.dart';
import '../../core/theme/design_tokens.dart';
import '../../desktop/widgets/update_banner.dart';
import '../dialogs/confirm_dialog.dart';
import '../state/trash_controller.dart';
import '../widgets/controls.dart';
import '../widgets/file_icon.dart';

/// 回收站：列表、恢复、永久删除（二次确认）与 30 天过期说明。
class TrashPage extends ConsumerWidget {
  const TrashPage({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = DesktopTokens.of(context);
    final state = ref.watch(trashControllerProvider);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          height: DesktopMetrics.toolbarHeight,
          padding: const EdgeInsets.symmetric(horizontal: 20),
          decoration: BoxDecoration(
            border: Border(bottom: BorderSide(color: tokens.line)),
          ),
          child: Row(
            children: [
              Text(
                '回收站',
                style: TextStyle(
                  color: tokens.ink900,
                  fontSize: tokens.textXl,
                  fontWeight: FontWeight.w600,
                ),
              ),
              const Spacer(),
              TokensIconButton(
                key: const Key('refresh-trash'),
                icon: LucideIcons.refreshCw,
                tooltip: '刷新',
                onPressed: () =>
                    ref.read(trashControllerProvider.notifier).refresh(),
              ),
            ],
          ),
        ),
        const UpdateBanner(),
        Expanded(child: _body(context, ref, state)),
      ],
    );
  }

  Widget _body(BuildContext context, WidgetRef ref, TrashState state) {
    final tokens = DesktopTokens.of(context);
    if (state.loading && state.items.isEmpty) {
      return Center(
        child: SizedBox(
          width: 22,
          height: 22,
          child: CircularProgressIndicator(
            strokeWidth: 2,
            color: tokens.brand600,
          ),
        ),
      );
    }
    if (state.error != null && state.items.isEmpty) {
      return Center(
        child: Text(
          state.error!,
          style: TextStyle(color: tokens.danger700, fontSize: tokens.textSm),
        ),
      );
    }
    if (state.items.isEmpty) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Container(
              width: 56,
              height: 56,
              decoration: BoxDecoration(
                color: tokens.canvas,
                shape: BoxShape.circle,
              ),
              child: Icon(LucideIcons.trash, size: 26, color: tokens.ink500),
            ),
            const SizedBox(height: 16),
            Text(
              '回收站是空的',
              style: TextStyle(
                color: tokens.ink900,
                fontSize: tokens.textLg,
                fontWeight: FontWeight.w600,
              ),
            ),
            const SizedBox(height: 6),
            Text(
              '删除的文件会先放在这里，可随时恢复。',
              style: TextStyle(color: tokens.ink500, fontSize: tokens.textSm),
            ),
          ],
        ),
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          height: DesktopMetrics.tableHeaderHeight,
          padding: const EdgeInsets.symmetric(horizontal: 20),
          decoration: BoxDecoration(
            border: Border(bottom: BorderSide(color: tokens.line)),
          ),
          child: Row(
            children: [
              Expanded(child: _header(tokens, '名称')),
              SizedBox(width: 180, child: _header(tokens, '删除时间')),
              SizedBox(width: 110, child: _header(tokens, '大小')),
              SizedBox(width: 176, child: _header(tokens, '')),
            ],
          ),
        ),
        Expanded(
          child: ListView.separated(
            itemCount: state.items.length,
            separatorBuilder: (context, index) =>
                Divider(color: tokens.line, height: 1),
            itemBuilder: (context, index) {
              final item = state.items[index];
              return _TrashRow(
                item: item,
                now: ref.read(clockProvider)(),
                busy: state.busy,
                onRestore: () => _restore(context, ref, item),
                onDelete: () => _permanentlyDelete(context, ref, item),
              );
            },
          ),
        ),
        Padding(
          padding: const EdgeInsets.all(16),
          child: TokensNote(
            icon: LucideIcons.info,
            text: '回收站中的项目会在 30 天后自动永久删除；在此期间仍占用存储空间。',
          ),
        ),
      ],
    );
  }

  Widget _header(DesktopTokens tokens, String label) {
    return Text(
      label,
      style: TextStyle(color: tokens.ink500, fontSize: tokens.textXs),
    );
  }

  Future<void> _restore(
    BuildContext context,
    WidgetRef ref,
    TrashItemDto item,
  ) async {
    await ref.read(trashControllerProvider.notifier).restore(item);
    if (!context.mounted) {
      return;
    }
    ScaffoldMessenger.maybeOf(context)?.showSnackBar(
      SnackBar(
        content: Text('“${item.node.name}”已恢复'),
        behavior: SnackBarBehavior.floating,
      ),
    );
  }

  Future<void> _permanentlyDelete(
    BuildContext context,
    WidgetRef ref,
    TrashItemDto item,
  ) async {
    final confirmed = await showConfirmDialog(
      context: context,
      title: '永久删除',
      message: '“${item.node.name}”及其所有版本将被永久删除，且无法恢复。',
      detail: '删除后不会进入回收站，也不能通过恢复密钥找回。',
      confirmLabel: '永久删除',
      danger: true,
      icon: LucideIcons.trash,
      iconWarn: false,
    );
    if (!confirmed || !context.mounted) {
      return;
    }
    final outcome = await ref
        .read(trashControllerProvider.notifier)
        .permanentlyDelete(item);
    if (!context.mounted) {
      return;
    }
    final message = outcome == null
        ? (ref.read(trashControllerProvider).error ?? '永久删除失败')
        : '已永久删除，释放 ${formatBytes(outcome.freedBytes)} 配额';
    ScaffoldMessenger.maybeOf(context)?.showSnackBar(
      SnackBar(content: Text(message), behavior: SnackBarBehavior.floating),
    );
  }
}

class _TrashRow extends StatelessWidget {
  const _TrashRow({
    required this.item,
    required this.now,
    required this.busy,
    required this.onRestore,
    required this.onDelete,
  });

  final TrashItemDto item;
  final DateTime now;
  final bool busy;
  final VoidCallback onRestore;
  final VoidCallback onDelete;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Container(
      key: ValueKey<String>('trash-item-${item.node.id}'),
      height: DesktopMetrics.tableRowHeight,
      padding: const EdgeInsets.symmetric(horizontal: 20),
      child: Row(
        children: [
          Expanded(
            child: Row(
              children: [
                FileIconTile.node(item.node),
                const SizedBox(width: 12),
                Expanded(
                  child: Text(
                    item.node.name,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      color: tokens.ink900,
                      fontSize: tokens.textSm,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                ),
              ],
            ),
          ),
          SizedBox(
            width: 180,
            child: Text(
              formatIsoTime(item.deletedAt, now: now),
              style: TextStyle(color: tokens.ink500, fontSize: tokens.textSm),
            ),
          ),
          SizedBox(
            width: 110,
            child: Text(
              item.node.kind == NodeKindDto.folder
                  ? '—'
                  : formatBytes(item.node.size),
              style: TextStyle(color: tokens.ink500, fontSize: tokens.textSm),
            ),
          ),
          SizedBox(
            width: 176,
            child: Row(
              mainAxisAlignment: MainAxisAlignment.end,
              children: [
                TokensButton(
                  key: ValueKey<String>('restore-${item.node.id}'),
                  label: '恢复',
                  icon: LucideIcons.rotateCcw,
                  onPressed: busy ? null : onRestore,
                ),
                const SizedBox(width: 8),
                TokensButton(
                  key: ValueKey<String>('permanent-${item.node.id}'),
                  label: '永久删除',
                  variant: TokensButtonVariant.ghost,
                  onPressed: busy ? null : onDelete,
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
