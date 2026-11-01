import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../common/widgets/controls.dart';
import '../../common/widgets/window_frame.dart';
import '../../core/format/format.dart';
import '../../core/providers/app_providers.dart';
import '../../core/theme/design_tokens.dart';
import '../state/update_controller.dart';
import '../widgets/update_actions.dart';

/// 强制更新阻断页（design/desktop/update.html `forced`）。
///
/// 当前版本低于已验签清单的最低支持版本时替换整个主界面路由；只提供
/// 下载并安装与打开下载页面，下载进度与失败重试都在页面内。备份引擎保持
/// 停止，不删除任何本地数据或凭据。
class ForcedUpdatePage extends ConsumerStatefulWidget {
  const ForcedUpdatePage({super.key});

  @override
  ConsumerState<ForcedUpdatePage> createState() => _ForcedUpdatePageState();
}

class _ForcedUpdatePageState extends ConsumerState<ForcedUpdatePage> {
  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final state = ref.watch(updateControllerProvider);
    final info = state.info;
    final version = info?.version ?? '';
    final currentVersion = ref.watch(appVersionProvider);
    final size = info == null ? '' : formatPackageBytes(info.size);
    return ColoredBox(
      color: tokens.designCanvas,
      child: WindowFrame(
        title: '加密备份',
        child: LayoutBuilder(
          builder: (context, constraints) {
            final sideWidth = constraints.maxWidth >= 700 ? 360.0 : 300.0;
            return Row(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                SizedBox(
                  width: sideWidth,
                  child: _SidePanel(version: version),
                ),
                Expanded(
                  child: Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 48),
                    child: Center(
                      child: Column(
                        mainAxisSize: MainAxisSize.min,
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text(
                            '需要更新后才能继续使用',
                            style: TextStyle(
                              color: tokens.ink900,
                              fontSize: 22,
                              fontWeight: FontWeight.w600,
                            ),
                          ),
                          const SizedBox(height: 14),
                          Text(
                            '当前版本 $currentVersion 已不再受支持。'
                            '更新不会影响你的文件、备份设置和登录状态；'
                            '更新完成前，备份已暂停。',
                            style: TextStyle(
                              color: tokens.ink500,
                              fontSize: tokens.textSm,
                              height: 1.55,
                            ),
                          ),
                          const SizedBox(height: 14),
                          TokensNote(
                            icon: LucideIcons.shieldCheck,
                            tone: NoteTone.info,
                            text:
                                '新版本 $version · $size · '
                                '安装包会先校验签名与 SHA-256。',
                          ),
                          const SizedBox(height: 14),
                          if (state.phase == UpdatePhase.downloading)
                            _downloadProgress(tokens, state)
                          else if (state.phase == UpdatePhase.completed)
                            _completed(tokens, version)
                          else if (state.phase == UpdatePhase.failed)
                            _failed(tokens, state)
                          else
                            _actions(tokens),
                        ],
                      ),
                    ),
                  ),
                ),
              ],
            );
          },
        ),
      ),
    );
  }

  Widget _actions(DesktopTokens tokens) {
    return Row(
      children: [
        TokensButton(
          key: const Key('forced-download'),
          label: '下载并安装',
          icon: LucideIcons.download,
          variant: TokensButtonVariant.primary,
          large: true,
          onPressed: () =>
              ref.read(updateControllerProvider.notifier).startDownload(),
        ),
        const SizedBox(width: 8),
        TokensButton(
          key: const Key('forced-download-page'),
          label: '打开下载页面',
          icon: LucideIcons.externalLink,
          variant: TokensButtonVariant.ghost,
          large: true,
          onPressed: () => openUpdateDownloadPage(context, ref),
        ),
      ],
    );
  }

  Widget _downloadProgress(DesktopTokens tokens, UpdateState state) {
    final progress = state.download;
    final downloaded = progress?.downloadedBytes ?? 0;
    final total = progress?.totalBytes ?? state.info?.size ?? 0;
    final fraction = total <= 0
        ? 0.0
        : (downloaded / total).clamp(0.0, 1.0).toDouble();
    final speed = formatSpeed(progress?.speedBytesPerSecond.toDouble());
    final detail = speed.isEmpty
        ? '${formatPackageBytes(downloaded)} / ${formatPackageBytes(total)}'
        : '${formatPackageBytes(downloaded)} / ${formatPackageBytes(total)} · $speed';
    return SizedBox(
      width: double.infinity,
      child: Column(
        key: const Key('forced-progress'),
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            '正在下载 ${(fraction * 100).round()}% · $detail',
            style: TextStyle(color: tokens.ink600, fontSize: tokens.textSm),
          ),
          const SizedBox(height: 10),
          Container(
            height: 6,
            decoration: BoxDecoration(
              color: tokens.canvasStrong,
              borderRadius: BorderRadius.circular(3),
            ),
            clipBehavior: Clip.antiAlias,
            child: Align(
              alignment: Alignment.centerLeft,
              child: FractionallySizedBox(
                widthFactor: fraction,
                child: Container(color: tokens.brand600),
              ),
            ),
          ),
          const SizedBox(height: 14),
          Row(
            children: [
              Expanded(
                child: Text(
                  '下载完成后会校验文件，备份会在更新完成后自动恢复。',
                  style: TextStyle(
                    color: tokens.ink500,
                    fontSize: tokens.textXs,
                  ),
                ),
              ),
              const SizedBox(width: 12),
              TokensButton(
                key: const Key('forced-cancel'),
                label: '取消',
                onPressed: () => ref
                    .read(updateControllerProvider.notifier)
                    .cancelDownload(),
              ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _completed(DesktopTokens tokens, String version) {
    return Column(
      key: const Key('forced-completed'),
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          '$version 已下载并通过校验，安装完成后会自动重新打开。',
          style: TextStyle(
            color: tokens.ink600,
            fontSize: tokens.textSm,
            height: 1.5,
          ),
        ),
        const SizedBox(height: 14),
        Row(
          children: [
            TokensButton(
              key: const Key('forced-restart-now'),
              label: '立即重新启动',
              icon: LucideIcons.refreshCw,
              variant: TokensButtonVariant.primary,
              large: true,
              onPressed: () => unawaited(runUpdateInstall(context, ref)),
            ),
            const SizedBox(width: 8),
            TokensButton(
              key: const Key('forced-download-page'),
              label: '打开下载页面',
              icon: LucideIcons.externalLink,
              variant: TokensButtonVariant.ghost,
              large: true,
              onPressed: () => openUpdateDownloadPage(context, ref),
            ),
          ],
        ),
      ],
    );
  }

  Widget _failed(DesktopTokens tokens, UpdateState state) {
    final message =
        state.failure ??
        state.download?.error?.message ??
        '下载的文件不完整或被篡改，已删除，没有运行。可能是网络中断或被安全软件修改。';
    return SizedBox(
      width: double.infinity,
      child: Column(
        key: const Key('forced-failed'),
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            message,
            style: TextStyle(
              color: tokens.danger700,
              fontSize: tokens.textSm,
              height: 1.5,
            ),
          ),
          const SizedBox(height: 14),
          Row(
            children: [
              TokensButton(
                key: const Key('forced-retry'),
                label: '重新下载',
                icon: LucideIcons.refreshCw,
                variant: TokensButtonVariant.primary,
                large: true,
                onPressed: () =>
                    ref.read(updateControllerProvider.notifier).startDownload(),
              ),
              const SizedBox(width: 8),
              TokensButton(
                key: const Key('forced-download-page'),
                label: '打开下载页面',
                icon: LucideIcons.externalLink,
                variant: TokensButtonVariant.ghost,
                large: true,
                onPressed: () => openUpdateDownloadPage(context, ref),
              ),
            ],
          ),
        ],
      ),
    );
  }
}

class _SidePanel extends StatelessWidget {
  const _SidePanel({required this.version});

  final String version;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return DecoratedBox(
      decoration: const BoxDecoration(
        gradient: LinearGradient(
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
          colors: <Color>[Color(0xFF0061FE), Color(0xFF1E3A8A)],
        ),
      ),
      child: Stack(
        children: [
          Positioned(
            left: -30,
            top: -60,
            child: _Ring(size: 420, color: tokens.onBrand),
          ),
          Positioned(
            left: 40,
            top: 10,
            child: _Ring(size: 280, color: tokens.onBrand),
          ),
          Align(
            alignment: const Alignment(0, -0.25),
            child: Container(
              width: 96,
              height: 96,
              decoration: BoxDecoration(
                color: tokens.onBrand.withValues(alpha: 0.14),
                borderRadius: BorderRadius.circular(24),
              ),
              child: Icon(
                LucideIcons.download,
                size: 44,
                color: tokens.onBrand,
              ),
            ),
          ),
          Align(
            alignment: Alignment.bottomLeft,
            child: Padding(
              padding: const EdgeInsets.all(32),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    '加密备份 $version',
                    style: TextStyle(
                      color: tokens.onBrand,
                      fontSize: tokens.textLg,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                  const SizedBox(height: 4),
                  Text(
                    '由维护者签名发布',
                    style: TextStyle(
                      color: tokens.onBrand.withValues(alpha: 0.8),
                      fontSize: tokens.textSm,
                    ),
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

class _Ring extends StatelessWidget {
  const _Ring({required this.size, required this.color});

  final double size;
  final Color color;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: size,
      height: size,
      decoration: BoxDecoration(
        shape: BoxShape.circle,
        border: Border.all(color: color.withValues(alpha: 0.18)),
      ),
    );
  }
}
