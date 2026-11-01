import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../common/widgets/controls.dart';
import '../../core/theme/design_tokens.dart';
import '../state/update_controller.dart';

/// 主窗口顶部更新横幅（design/desktop/update.html `banner`）。
///
/// 放在各主页面工具栏下方、内容上方；没有更新时返回零尺寸，不占位，也不
/// 改变已有 goldens。
class UpdateBanner extends ConsumerWidget {
  const UpdateBanner({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = DesktopTokens.of(context);
    final state = ref.watch(updateControllerProvider);
    final info = state.info;
    if (!state.bannerVisible || info == null) {
      return const SizedBox.shrink();
    }
    final notes = info.notes.join('；');
    final summary = notes.isEmpty
        ? '更新约需 30 秒，完成后自动重新打开。'
        : '$notes。更新约需 30 秒，完成后自动重新打开。';
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 16, 20, 0),
      child: Container(
        key: const Key('update-banner'),
        padding: const EdgeInsets.fromLTRB(14, 10, 12, 10),
        decoration: BoxDecoration(
          gradient: LinearGradient(
            begin: Alignment.centerLeft,
            end: Alignment.centerRight,
            colors: <Color>[tokens.brand50, tokens.surface],
            stops: const <double>[0, 0.7],
          ),
          border: Border.all(color: tokens.brand100),
          borderRadius: BorderRadius.circular(10),
        ),
        child: Row(
          children: [
            Container(
              width: 32,
              height: 32,
              decoration: BoxDecoration(
                color: tokens.brand600,
                borderRadius: BorderRadius.circular(8),
              ),
              child: Icon(
                LucideIcons.download,
                size: 16,
                color: tokens.onBrand,
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    '新版本 ${info.version} 可用',
                    style: TextStyle(
                      color: tokens.ink900,
                      fontSize: tokens.textSm,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                  const SizedBox(height: 2),
                  Text(
                    summary,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      color: tokens.ink500,
                      fontSize: tokens.textXs,
                    ),
                  ),
                ],
              ),
            ),
            const SizedBox(width: 12),
            TokensButton(
              key: const Key('update-banner-notes'),
              label: '更新内容',
              variant: TokensButtonVariant.ghost,
              onPressed: () => context.push('/settings?tab=about'),
            ),
            const SizedBox(width: 8),
            TokensButton(
              key: const Key('update-banner-download'),
              label: '下载并安装',
              variant: TokensButtonVariant.primary,
              onPressed: () => _downloadAndOpenAbout(context, ref),
            ),
            const SizedBox(width: 4),
            TokensIconButton(
              key: const Key('update-banner-close'),
              icon: LucideIcons.x,
              tooltip: '关闭',
              onPressed: () => unawaited(
                ref.read(updateControllerProvider.notifier).dismissBanner(),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Future<void> _downloadAndOpenAbout(
    BuildContext context,
    WidgetRef ref,
  ) async {
    final download = ref
        .read(updateControllerProvider.notifier)
        .startDownload();
    if (context.mounted) {
      context.push('/settings?tab=about');
    }
    await download;
  }
}
