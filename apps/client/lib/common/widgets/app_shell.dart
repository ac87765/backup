import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';

import '../../core/router/destinations.dart';
import '../../core/theme/design_tokens.dart';

/// 主窗口骨架：左侧 232px 导航 + 顶部工具栏 + 内容 + 状态栏。
///
/// 窗口外框（Windows 11 标题栏 / macOS 红绿灯）在第 6 组实现；这里只保证
/// 内容布局与 design/desktop/files.html 的结构一致。
class AppShell extends StatelessWidget {
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
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final active = destinations.firstWhere(
      (destination) => location.startsWith(destination.path),
      orElse: () => destinations.first,
    );

    return Scaffold(
      backgroundColor: tokens.surface,
      body: Row(
        children: [
          _Sidebar(
            location: location,
            destinations: destinations,
            tokens: tokens,
          ),
          Expanded(
            child: Column(
              children: [
                _Toolbar(title: active.label, tokens: tokens),
                Expanded(child: child),
                _StatusBar(tokens: tokens),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _Sidebar extends StatelessWidget {
  const _Sidebar({
    required this.location,
    required this.destinations,
    required this.tokens,
  });

  final String location;
  final List<AppDestination> destinations;
  final DesktopTokens tokens;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: 232,
      decoration: BoxDecoration(
        color: tokens.canvas,
        border: Border(right: BorderSide(color: tokens.line)),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: EdgeInsets.fromLTRB(
              tokens.space4,
              tokens.space4,
              tokens.space4,
              tokens.space4,
            ),
            child: Row(
              children: [
                Container(
                  width: 28,
                  height: 28,
                  decoration: BoxDecoration(
                    color: tokens.brand600,
                    borderRadius: BorderRadius.circular(8),
                  ),
                  child: const Icon(
                    Icons.lock_outline,
                    color: Colors.white,
                    size: 16,
                  ),
                ),
                SizedBox(width: tokens.space2),
                Text('加密备份', style: Theme.of(context).textTheme.titleMedium),
              ],
            ),
          ),
          Expanded(
            child: ListView(
              padding: EdgeInsets.symmetric(horizontal: tokens.space3),
              children: [
                for (final destination in destinations)
                  _NavItem(
                    destination: destination,
                    selected: location.startsWith(destination.path),
                    tokens: tokens,
                  ),
              ],
            ),
          ),
          Divider(color: tokens.line, height: 1),
          Padding(
            padding: EdgeInsets.all(tokens.space3),
            child: Row(
              children: [
                CircleAvatar(
                  radius: 14,
                  backgroundColor: tokens.brand100,
                  child: Text(
                    '未',
                    style: TextStyle(
                      color: tokens.brand700,
                      fontSize: tokens.textXs,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                ),
                SizedBox(width: tokens.space2),
                Expanded(
                  child: Text(
                    '未登录',
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _NavItem extends StatelessWidget {
  const _NavItem({
    required this.destination,
    required this.selected,
    required this.tokens,
  });

  final AppDestination destination;
  final bool selected;
  final DesktopTokens tokens;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 2),
      child: Material(
        color: selected ? tokens.brand50 : Colors.transparent,
        borderRadius: BorderRadius.circular(tokens.radiusControl),
        child: InkWell(
          borderRadius: BorderRadius.circular(tokens.radiusControl),
          onTap: () => context.go(destination.path),
          child: SizedBox(
            height: 34,
            child: Padding(
              padding: EdgeInsets.symmetric(horizontal: tokens.space3),
              child: Row(
                children: [
                  Icon(
                    destination.icon,
                    size: 18,
                    color: selected ? tokens.brand700 : tokens.ink600,
                  ),
                  SizedBox(width: tokens.space2),
                  Text(
                    destination.label,
                    style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                      color: selected ? tokens.brand700 : tokens.ink600,
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

class _Toolbar extends StatelessWidget {
  const _Toolbar({required this.title, required this.tokens});

  final String title;
  final DesktopTokens tokens;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 56,
      padding: EdgeInsets.symmetric(horizontal: tokens.space5),
      decoration: BoxDecoration(
        color: tokens.surface,
        border: Border(bottom: BorderSide(color: tokens.line)),
      ),
      child: Row(
        children: [
          Text(title, style: Theme.of(context).textTheme.titleLarge),
          const Spacer(),
          Container(
            height: 28,
            padding: EdgeInsets.symmetric(horizontal: tokens.space3),
            decoration: BoxDecoration(
              color: tokens.canvas,
              borderRadius: BorderRadius.circular(tokens.radiusControl),
              border: Border.all(color: tokens.line),
            ),
            child: Center(
              child: Text(
                '第 4 组：工程骨架',
                style: Theme.of(context).textTheme.bodySmall,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _StatusBar extends StatelessWidget {
  const _StatusBar({required this.tokens});

  final DesktopTokens tokens;

  @override
  Widget build(BuildContext context) {
    return Container(
      height: 32,
      padding: EdgeInsets.symmetric(horizontal: tokens.space4),
      decoration: BoxDecoration(
        color: tokens.canvas,
        border: Border(top: BorderSide(color: tokens.line)),
      ),
      child: Row(
        children: [
          Container(
            width: 8,
            height: 8,
            decoration: BoxDecoration(
              color: tokens.success600,
              shape: BoxShape.circle,
            ),
          ),
          SizedBox(width: tokens.space2),
          Text('加密备份', style: Theme.of(context).textTheme.bodySmall),
          const Spacer(),
          Text(
            '窗口 1280×800 · 最小 960×600',
            style: Theme.of(context).textTheme.bodySmall,
          ),
        ],
      ),
    );
  }
}
