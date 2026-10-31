import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/format/format.dart';
import '../../core/providers/app_providers.dart';
import '../../core/router/destinations.dart';
import '../../core/rust/models.dart';
import '../../core/theme/design_tokens.dart';
import '../state/auth_controller.dart';
import '../state/transfer_controller.dart';
import 'controls.dart';
import 'transfer_panel.dart';
import 'window_frame.dart';

/// 主窗口骨架：左侧 232px 导航 + 页面内容 + 状态栏。
///
/// 页面自己负责顶部工具栏，因为文件页的工具栏（面包屑、筛选、上传）与设置类
/// 页面不同；状态栏在这里统一渲染。
class AppShell extends ConsumerWidget {
  const AppShell({
    required this.location,
    required this.destinations,
    required this.child,
    super.key,
  });

  final String location;
  final List<AppDestination> destinations;
  final Widget child;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = DesktopTokens.of(context);
    final panelOpen = ref.watch(transferPanelOpenProvider);
    return Scaffold(
      backgroundColor: tokens.surface,
      body: WindowFrame(
        child: Row(
          children: [
            _Sidebar(
              location: location,
              destinations: destinations,
              tokens: tokens,
            ),
            Expanded(
              child: Column(
                children: [
                  Expanded(
                    child: Stack(
                      children: [
                        Positioned.fill(child: child),
                        if (panelOpen)
                          Positioned(
                            right: tokens.space4,
                            bottom: tokens.space3,
                            width: DesktopMetrics.transferPanelWidth,
                            child: const TransferPanel(),
                          ),
                      ],
                    ),
                  ),
                  _StatusBar(tokens: tokens),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _Sidebar extends ConsumerWidget {
  const _Sidebar({
    required this.location,
    required this.destinations,
    required this.tokens,
  });

  final String location;
  final List<AppDestination> destinations;
  final DesktopTokens tokens;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final auth = ref.watch(authControllerProvider);
    final usage = ref.watch(usageProvider);
    final email = auth.account?.email ?? '';
    final initial = email.isEmpty ? '?' : email.characters.first.toUpperCase();
    final quota = usage.value;
    final used = quota?.usedBytes ?? 0;
    final total = quota?.quotaBytes ?? 0;
    final fraction = total <= 0
        ? 0.0
        : (used / total).clamp(0.0, 1.0).toDouble();
    final securityIndex = destinations.indexWhere(
      (destination) => destination.path == '/security',
    );
    return Container(
      width: DesktopMetrics.sidebarWidth,
      padding: const EdgeInsets.only(top: 8, left: 12, right: 12, bottom: 12),
      decoration: BoxDecoration(
        color: tokens.canvas,
        border: Border(right: BorderSide(color: tokens.line)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(8, 8, 8, 16),
            child: Row(
              children: [
                const BrandLogo(size: DesktopMetrics.brandLogoSize),
                const SizedBox(width: 10),
                Text(
                  '加密备份',
                  style: TextStyle(
                    color: tokens.ink900,
                    fontSize: tokens.textMd,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ],
            ),
          ),
          for (var index = 0; index < destinations.length; index += 1) ...[
            if (index == securityIndex && securityIndex >= 0)
              Padding(
                padding: const EdgeInsets.symmetric(
                  horizontal: 4,
                  vertical: 10,
                ),
                child: Container(height: 1, color: tokens.line),
              ),
            _NavItem(
              destination: destinations[index],
              selected: _isSelected(destinations[index].path),
              tokens: tokens,
            ),
            if (index != destinations.length - 1 && index + 1 != securityIndex)
              const SizedBox(height: 2),
          ],
          const Spacer(),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 12),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [
                    Text(
                      '存储空间',
                      style: TextStyle(
                        color: tokens.ink500,
                        fontSize: tokens.textXs,
                      ),
                    ),
                    const SizedBox(width: 8),
                    Flexible(
                      child: Text(
                        quota == null
                            ? '—'
                            : '${formatBytes(used)} / ${formatBytes(total)}',
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        textAlign: TextAlign.right,
                        style: TextStyle(
                          color: tokens.ink500,
                          fontSize: tokens.textXs,
                        ),
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 6),
                Container(
                  height: DesktopMetrics.quotaBarHeight,
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
              ],
            ),
          ),
          Container(
            padding: const EdgeInsets.all(8),
            decoration: BoxDecoration(
              border: Border(top: BorderSide(color: tokens.line)),
            ),
            child: Row(
              children: [
                Container(
                  width: DesktopMetrics.avatarSize,
                  height: DesktopMetrics.avatarSize,
                  alignment: Alignment.center,
                  decoration: BoxDecoration(
                    color: tokens.brand100,
                    shape: BoxShape.circle,
                  ),
                  child: Text(
                    initial,
                    style: TextStyle(
                      color: tokens.brand700,
                      fontSize: tokens.textXs,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        email,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          color: tokens.ink900,
                          fontSize: tokens.textSm,
                          fontWeight: FontWeight.w500,
                        ),
                      ),
                      const SizedBox(height: 1),
                      Text(
                        '已加密 · 本机已解锁',
                        style: TextStyle(
                          color: tokens.ink500,
                          fontSize: tokens.textXs,
                        ),
                      ),
                    ],
                  ),
                ),
                TokensIconButton(
                  key: const Key('open-settings'),
                  icon: LucideIcons.settings,
                  tooltip: '设置',
                  onPressed: () => context.push('/settings'),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  bool _isSelected(String path) {
    if (path == '/files') {
      return location == '/files' || location.startsWith('/files/');
    }
    return location == path || location.startsWith('$path/');
  }
}

class _NavItem extends StatefulWidget {
  const _NavItem({
    required this.destination,
    required this.selected,
    required this.tokens,
  });

  final AppDestination destination;
  final bool selected;
  final DesktopTokens tokens;

  @override
  State<_NavItem> createState() => _NavItemState();
}

class _NavItemState extends State<_NavItem> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final tokens = widget.tokens;
    final selected = widget.selected;
    final color = selected ? tokens.brand700 : tokens.ink600;
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: Material(
        color: selected
            ? tokens.brand50
            : _hover
            ? tokens.canvasStrong
            : Colors.transparent,
        borderRadius: BorderRadius.circular(tokens.radiusControl),
        child: InkWell(
          borderRadius: BorderRadius.circular(tokens.radiusControl),
          onTap: () => context.go(widget.destination.path),
          child: SizedBox(
            height: DesktopMetrics.navItemHeight,
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 10),
              child: Row(
                children: [
                  Icon(widget.destination.icon, size: 16, color: color),
                  const SizedBox(width: 10),
                  Text(
                    widget.destination.label,
                    style: TextStyle(
                      color: color,
                      fontSize: tokens.textSm,
                      fontWeight: selected ? FontWeight.w600 : FontWeight.w400,
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _StatusBar extends ConsumerStatefulWidget {
  const _StatusBar({required this.tokens});

  final DesktopTokens tokens;

  @override
  ConsumerState<_StatusBar> createState() => _StatusBarState();
}

class _StatusBarState extends ConsumerState<_StatusBar> {
  BackupSummaryDto? _latest;

  @override
  Widget build(BuildContext context) {
    final tokens = widget.tokens;
    final transfers = ref.watch(transferControllerProvider);
    final initial = ref.watch(backupSummaryProvider).value;
    ref.listen(backupEventsProvider, (previous, next) {
      next.whenData((event) {
        final summary = event.summary;
        if (summary != null && summary != _latest) {
          setState(() => _latest = summary);
        }
      });
    });
    final summary = _latest ?? initial;
    final (color, text) = _backupLabel(tokens, summary);
    final active = transfers.activeCount;
    return Container(
      height: DesktopMetrics.statusBarHeight,
      padding: const EdgeInsets.symmetric(horizontal: 16),
      decoration: BoxDecoration(
        color: tokens.canvas,
        border: Border(top: BorderSide(color: tokens.line)),
      ),
      child: Row(
        children: [
          Container(
            width: 8,
            height: 8,
            decoration: BoxDecoration(color: color, shape: BoxShape.circle),
          ),
          const SizedBox(width: 6),
          Text(
            text,
            style: TextStyle(color: tokens.ink600, fontSize: tokens.textXs),
          ),
          const Spacer(),
          InkWell(
            key: const Key('open-transfers'),
            onTap: () => ref.read(transferPanelOpenProvider.notifier).toggle(),
            borderRadius: BorderRadius.circular(4),
            child: Padding(
              padding: const EdgeInsets.symmetric(vertical: 2, horizontal: 2),
              child: Row(
                children: [
                  Icon(
                    LucideIcons.arrowUpDown,
                    size: 14,
                    color: tokens.brand700,
                  ),
                  const SizedBox(width: 6),
                  Text(
                    active > 0 ? '传输 · $active 项进行中' : '传输',
                    style: TextStyle(
                      color: tokens.brand700,
                      fontSize: tokens.textXs,
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

  (Color, String) _backupLabel(
    DesktopTokens tokens,
    BackupSummaryDto? summary,
  ) {
    if (summary == null) {
      return (tokens.ink400, '备份状态不可用');
    }
    if (summary.folderCount == 0) {
      return (tokens.ink400, '还没有备份文件夹');
    }
    return switch (summary.kind) {
      BackupSummaryStatusKindDto.allBackedUp => (
        tokens.success600,
        '已全部备份 · ${summary.folderCount} 个备份文件夹',
      ),
      BackupSummaryStatusKindDto.uploading => (
        tokens.brand600,
        '正在备份 · 剩余 ${summary.total - summary.done} 个文件',
      ),
      BackupSummaryStatusKindDto.paused => (tokens.warning600, '备份已暂停'),
      BackupSummaryStatusKindDto.needsUnlock => (
        tokens.warning600,
        '需要解锁才能继续备份',
      ),
      BackupSummaryStatusKindDto.error => (tokens.danger600, '备份遇到问题'),
    };
  }
}
