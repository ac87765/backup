import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/config/app_config.dart';
import '../../core/platform/external_links.dart';
import '../../core/providers/app_providers.dart';
import '../../core/rust/models.dart';
import '../../core/theme/design_tokens.dart';
import '../../desktop/startup/startup_service.dart';
import '../dialogs/keep_unlocked_dialog.dart';
import '../state/auth_controller.dart';
import '../widgets/app_dialog.dart';
import '../widgets/controls.dart';
import '../widgets/popover_menu.dart';
import '../widgets/window_frame.dart';

enum SettingsTab { general, security, account, about }

/// 设置窗口（settings.html 的 general / security / confirm / about 状态）。
class SettingsPage extends ConsumerStatefulWidget {
  const SettingsPage({super.key});

  @override
  ConsumerState<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends ConsumerState<SettingsPage> {
  SettingsTab _tab = SettingsTab.general;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final auth = ref.watch(authControllerProvider);
    final config = ref.watch(appConfigProvider).value;
    return AppWindowScaffold(
      width: DesktopMetrics.settingsWindowWidth,
      height: DesktopMetrics.settingsWindowHeight,
      title: '设置',
      onClose: () {
        if (context.canPop()) {
          context.pop();
        } else {
          context.go('/files');
        }
      },
      child: Row(
        children: [
          Container(
            width: DesktopMetrics.settingsTabsWidth,
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 16),
            decoration: BoxDecoration(
              color: tokens.canvas,
              border: Border(right: BorderSide(color: tokens.line)),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                _TabItem(
                  label: '通用',
                  icon: LucideIcons.sliders,
                  selected: _tab == SettingsTab.general,
                  onTap: () => setState(() => _tab = SettingsTab.general),
                ),
                const SizedBox(height: 2),
                _TabItem(
                  label: '安全',
                  icon: LucideIcons.shield,
                  selected: _tab == SettingsTab.security,
                  onTap: () => setState(() => _tab = SettingsTab.security),
                ),
                const SizedBox(height: 2),
                _TabItem(
                  label: '账号',
                  icon: LucideIcons.user,
                  selected: _tab == SettingsTab.account,
                  onTap: () => setState(() => _tab = SettingsTab.account),
                ),
                const SizedBox(height: 2),
                _TabItem(
                  label: '关于',
                  icon: LucideIcons.info,
                  selected: _tab == SettingsTab.about,
                  onTap: () => setState(() => _tab = SettingsTab.about),
                ),
              ],
            ),
          ),
          Expanded(
            child: SingleChildScrollView(
              padding: const EdgeInsets.symmetric(horizontal: 32, vertical: 24),
              child: switch (_tab) {
                SettingsTab.general => _GeneralPanel(tokens: tokens),
                SettingsTab.security => _SecurityPanel(
                  tokens: tokens,
                  auth: auth,
                  onToggleKeepUnlocked: _toggleKeepUnlocked,
                  onLogout: _logout,
                ),
                SettingsTab.account => _AccountPanel(
                  tokens: tokens,
                  auth: auth,
                  config: config,
                  onLogout: _logout,
                ),
                SettingsTab.about => _AboutPanel(
                  tokens: tokens,
                  config: config,
                  onOpenSecurity: () => context.go('/security'),
                  onOpenLicenses: _showLicenses,
                  onDownload: _downloadLatest,
                ),
              },
            ),
          ),
        ],
      ),
    );
  }

  Future<void> _toggleKeepUnlocked(bool enable) async {
    final auth = ref.read(authControllerProvider.notifier);
    if (!enable) {
      await auth.disableKeepUnlocked();
      return;
    }
    await showKeepUnlockedDialog(context);
  }

  Future<void> _logout() async {
    await ref.read(authControllerProvider.notifier).logout();
  }

  Future<void> _downloadLatest() async {
    if (releasesUrl.isEmpty) {
      _showMessage('尚未配置下载地址');
      return;
    }
    await ref.read(externalLinkOpenerProvider)(Uri.parse(releasesUrl));
  }

  Future<void> _showLicenses() {
    return showTokensDialog<void>(
      context: context,
      builder: (dialogContext) {
        final dialogTokens = DesktopTokens.of(dialogContext);
        return TokensDialogShell(
          title: '开源许可',
          width: DesktopMetrics.dialogWidthSmall,
          onClose: () => Navigator.of(dialogContext).pop(),
          body: Text(
            '加密备份使用以下开源软件：\n\n'
            '• Flutter / Dart（BSD-3-Clause）\n'
            '• flutter_rust_bridge（MIT）\n'
            '• Riverpod（MIT）\n'
            '• Lucide Icons（ISC）\n'
            '• file_selector / url_launcher / path_provider（BSD-3-Clause）\n'
            '• Rust crates：Apache-2.0 或 MIT（详见 Cargo.lock 与各 crate 许可证）',
            style: TextStyle(
              color: dialogTokens.ink600,
              fontSize: dialogTokens.textSm,
              height: 1.6,
            ),
          ),
          actions: [
            TokensButton(
              label: '关闭',
              onPressed: () => Navigator.of(dialogContext).pop(),
            ),
          ],
        );
      },
    );
  }

  void _showMessage(String message) {
    ScaffoldMessenger.maybeOf(context)?.showSnackBar(
      SnackBar(content: Text(message), behavior: SnackBarBehavior.floating),
    );
  }
}

class _TabItem extends StatelessWidget {
  const _TabItem({
    required this.label,
    required this.icon,
    required this.selected,
    required this.onTap,
  });

  final String label;
  final IconData icon;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final color = selected ? tokens.brand700 : tokens.ink600;
    return Material(
      color: selected ? tokens.brand50 : Colors.transparent,
      borderRadius: BorderRadius.circular(tokens.radiusControl),
      child: InkWell(
        borderRadius: BorderRadius.circular(tokens.radiusControl),
        onTap: onTap,
        child: SizedBox(
          height: DesktopMetrics.navItemHeight,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 10),
            child: Row(
              children: [
                Icon(icon, size: 16, color: color),
                const SizedBox(width: 10),
                Text(
                  label,
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
    );
  }
}

class _GeneralPanel extends ConsumerStatefulWidget {
  const _GeneralPanel({required this.tokens});

  final DesktopTokens tokens;

  @override
  ConsumerState<_GeneralPanel> createState() => _GeneralPanelState();
}

class _GeneralPanelState extends ConsumerState<_GeneralPanel> {
  bool _busy = false;

  static const _speeds = <_SpeedOption>[
    _SpeedOption(label: '不限制', bytesPerSecond: null),
    _SpeedOption(label: '1 MB/s', bytesPerSecond: 1000000),
    _SpeedOption(label: '5 MB/s', bytesPerSecond: 5000000),
    _SpeedOption(label: '10 MB/s', bytesPerSecond: 10000000),
  ];

  Future<void> _setStartup(bool enabled) async {
    setState(() => _busy = true);
    try {
      await ref.read(startupServiceProvider).setEnabled(enabled);
      ref.invalidate(startupEnabledProvider);
    } finally {
      if (mounted) {
        setState(() => _busy = false);
      }
    }
  }

  Future<void> _setSpeed(int? bytesPerSecond) async {
    final settings = ref.read(backupSettingsProvider).value;
    if (settings == null) {
      return;
    }
    await _save(
      BackupSettingsDto(
        includeHidden: settings.includeHidden,
        uploadLimitBytesPerSecond: bytesPerSecond,
        paused: settings.paused,
        wechatSnapshotRetention: settings.wechatSnapshotRetention,
      ),
    );
  }

  Future<void> _setIncludeHidden(bool includeHidden) async {
    final settings = ref.read(backupSettingsProvider).value;
    if (settings == null) {
      return;
    }
    await _save(
      BackupSettingsDto(
        includeHidden: includeHidden,
        uploadLimitBytesPerSecond: settings.uploadLimitBytesPerSecond,
        paused: settings.paused,
        wechatSnapshotRetention: settings.wechatSnapshotRetention,
      ),
    );
  }

  Future<void> _setRetention(int retention) async {
    final settings = ref.read(backupSettingsProvider).value;
    if (settings == null) {
      return;
    }
    await _save(
      BackupSettingsDto(
        includeHidden: settings.includeHidden,
        uploadLimitBytesPerSecond: settings.uploadLimitBytesPerSecond,
        paused: settings.paused,
        wechatSnapshotRetention: retention.clamp(1, 10).toInt(),
      ),
    );
  }

  Future<void> _save(BackupSettingsDto settings) async {
    setState(() => _busy = true);
    try {
      await ref.read(nativeBridgeProvider).backupSetSettings(settings);
      ref.invalidate(backupSettingsProvider);
    } finally {
      if (mounted) {
        setState(() => _busy = false);
      }
    }
  }

  String _speedLabel(int? bytesPerSecond) {
    for (final speed in _speeds) {
      if (speed.bytesPerSecond == bytesPerSecond) {
        return speed.label;
      }
    }
    return '不限制';
  }

  @override
  Widget build(BuildContext context) {
    final tokens = widget.tokens;
    final settings = ref.watch(backupSettingsProvider).value;
    final startup = ref.watch(startupEnabledProvider).value ?? false;
    final isMac = defaultTargetPlatform == TargetPlatform.macOS;
    final trayName = isMac ? '屏幕顶部的菜单栏' : '任务栏右下角的托盘';
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _PanelTitle(tokens: tokens, text: '通用'),
        _GroupTitle(tokens: tokens, text: '启动'),
        _SettingRow(
          tokens: tokens,
          title: '开机时自动启动',
          description: '登录系统后在后台启动，继续备份。',
          control: TokensToggle(
            key: const Key('startup-toggle'),
            value: startup,
            onChanged: _busy ? null : _setStartup,
          ),
        ),
        _SettingRow(
          tokens: tokens,
          title: '关闭窗口时',
          description: '加密备份会留在$trayName里继续工作。要完全退出，请在托盘菜单中选择“退出”。',
        ),
        _GroupTitle(tokens: tokens, text: '备份'),
        _SettingRow(
          tokens: tokens,
          title: '上传速度限制',
          description: '限制后台备份占用的带宽，手动上传不受影响。',
          control: _SpeedSelect(
            key: const Key('speed-limit-select'),
            tokens: tokens,
            label: _speedLabel(settings?.uploadLimitBytesPerSecond),
            enabled: settings != null && !_busy,
            options: _speeds,
            onSelected: _setSpeed,
          ),
        ),
        _SettingRow(
          tokens: tokens,
          title: '备份隐藏文件',
          description: '以“.”开头或带隐藏属性的文件。系统与临时文件始终不备份。',
          control: TokensToggle(
            key: const Key('backup-hidden-toggle'),
            value: settings?.includeHidden ?? false,
            onChanged: settings == null || _busy ? null : _setIncludeHidden,
          ),
        ),
        _SettingRow(
          tokens: tokens,
          title: '聊天记录快照保留份数',
          description: '每个微信账号保留最近几份完整的聊天记录数据库快照；更早的移入回收站。',
          control: _RetentionSelect(
            key: const Key('wechat-retention-select'),
            tokens: tokens,
            value: settings?.wechatSnapshotRetention ?? 3,
            enabled: settings != null && !_busy,
            onSelected: _setRetention,
          ),
        ),
      ],
    );
  }
}

class _SpeedOption {
  const _SpeedOption({required this.label, required this.bytesPerSecond});

  final String label;
  final int? bytesPerSecond;
}

class _SpeedSelect extends StatelessWidget {
  const _SpeedSelect({
    required this.tokens,
    required this.label,
    required this.enabled,
    required this.options,
    required this.onSelected,
    super.key,
  });

  final DesktopTokens tokens;
  final String label;
  final bool enabled;
  final List<_SpeedOption> options;
  final ValueChanged<int?> onSelected;

  @override
  Widget build(BuildContext context) {
    return Builder(
      builder: (buttonContext) => Opacity(
        opacity: enabled ? 1 : 0.55,
        child: InkWell(
          onTap: enabled
              ? () => showTokensMenu(
                  context: context,
                  anchor: anchorOf(buttonContext),
                  items: [
                    for (final option in options)
                      TokensMenuItem(
                        label: option.label,
                        icon: option.label == label
                            ? LucideIcons.check
                            : LucideIcons.minus,
                        onSelected: () => onSelected(option.bytesPerSecond),
                      ),
                  ],
                )
              : null,
          borderRadius: BorderRadius.circular(tokens.radiusControl),
          child: Container(
            height: DesktopMetrics.buttonHeight,
            padding: const EdgeInsets.symmetric(horizontal: 10),
            decoration: BoxDecoration(
              border: Border.all(color: tokens.lineStrong),
              borderRadius: BorderRadius.circular(tokens.radiusControl),
            ),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(
                  label,
                  style: TextStyle(
                    color: tokens.ink900,
                    fontSize: tokens.textSm,
                  ),
                ),
                const SizedBox(width: 8),
                Icon(LucideIcons.chevronDown, size: 14, color: tokens.ink500),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _RetentionSelect extends StatelessWidget {
  const _RetentionSelect({
    required this.tokens,
    required this.value,
    required this.enabled,
    required this.onSelected,
    super.key,
  });

  final DesktopTokens tokens;
  final int value;
  final bool enabled;
  final ValueChanged<int> onSelected;

  @override
  Widget build(BuildContext context) {
    final current = value.clamp(1, 10).toInt();
    return Builder(
      builder: (buttonContext) => Opacity(
        opacity: enabled ? 1 : 0.55,
        child: InkWell(
          onTap: enabled
              ? () => showTokensMenu(
                  context: context,
                  anchor: anchorOf(buttonContext),
                  items: [
                    for (var count = 1; count <= 10; count += 1)
                      TokensMenuItem(
                        label: '$count 份',
                        icon: count == current
                            ? LucideIcons.check
                            : LucideIcons.minus,
                        onSelected: () => onSelected(count),
                      ),
                  ],
                )
              : null,
          borderRadius: BorderRadius.circular(tokens.radiusControl),
          child: Container(
            height: DesktopMetrics.buttonHeight,
            padding: const EdgeInsets.symmetric(horizontal: 10),
            decoration: BoxDecoration(
              border: Border.all(color: tokens.lineStrong),
              borderRadius: BorderRadius.circular(tokens.radiusControl),
            ),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(
                  '$current 份',
                  style: TextStyle(
                    color: tokens.ink900,
                    fontSize: tokens.textSm,
                  ),
                ),
                const SizedBox(width: 8),
                Icon(LucideIcons.chevronDown, size: 14, color: tokens.ink500),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _SecurityPanel extends StatelessWidget {
  const _SecurityPanel({
    required this.tokens,
    required this.auth,
    required this.onToggleKeepUnlocked,
    required this.onLogout,
  });

  final DesktopTokens tokens;
  final AuthState auth;
  final ValueChanged<bool> onToggleKeepUnlocked;
  final VoidCallback onLogout;

  @override
  Widget build(BuildContext context) {
    final keychain = defaultTargetPlatform == TargetPlatform.macOS
        ? 'macOS 钥匙串'
        : 'Windows 凭据保护';
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _PanelTitle(tokens: tokens, text: '安全'),
        _SettingRow(
          tokens: tokens,
          title: '在这台电脑上保持解锁',
          description:
              '重启电脑后无需输入密码即可继续备份。密钥由 $keychain 保管，'
              '只有这台电脑的当前系统账号能读取。',
          control: TokensToggle(
            key: const Key('keep-unlocked-toggle'),
            value: auth.keepUnlocked,
            onChanged: auth.busy ? null : onToggleKeepUnlocked,
          ),
        ),
        _SettingRow(
          tokens: tokens,
          title: '空闲时锁定',
          description: '未开启“保持解锁”时，应用窗口空闲一段时间后需要重新输入密码；后台备份不受影响。',
          control: const _SelectPlaceholder(label: '从不'),
        ),
        _GroupTitle(tokens: tokens, text: '这台电脑'),
        _SettingRow(
          tokens: tokens,
          title: auth.account?.email ?? '本机',
          description: '本机只保存文件路径与备份记录，不保存文件内容；云端看不到这些路径。',
          control: Tooltip(
            message: '本地记录清理将在桌面专有功能中提供',
            child: TokensButton(label: '清除本机记录…', onPressed: null),
          ),
        ),
        _SettingRow(
          tokens: tokens,
          title: '退出登录',
          description: '删除这台电脑上保存的登录状态与密钥。已备份的文件不受影响。',
          control: TokensButton(
            key: const Key('settings-logout'),
            label: '退出登录',
            variant: TokensButtonVariant.ghost,
            onPressed: auth.busy ? null : onLogout,
          ),
        ),
      ],
    );
  }
}

class _AccountPanel extends StatelessWidget {
  const _AccountPanel({
    required this.tokens,
    required this.auth,
    required this.config,
    required this.onLogout,
  });

  final DesktopTokens tokens;
  final AuthState auth;
  final AppConfig? config;
  final VoidCallback onLogout;

  @override
  Widget build(BuildContext context) {
    final account = auth.account;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _PanelTitle(tokens: tokens, text: '账号'),
        SizedBox(height: tokens.space4),
        Text(
          account?.email ?? '—',
          style: TextStyle(
            color: tokens.ink900,
            fontSize: tokens.textLg,
            fontWeight: FontWeight.w600,
          ),
        ),
        SizedBox(height: tokens.space2),
        Row(
          children: [
            TokensPill(
              label: account == null
                  ? '未知状态'
                  : account.status == AccountStatusDto.pending
                  ? '待审核'
                  : '已启用',
              tone: account?.status == AccountStatusDto.pending
                  ? PillTone.warn
                  : PillTone.ok,
            ),
            SizedBox(width: tokens.space2),
            TokensPill(
              label: '本机已解锁',
              tone: PillTone.busy,
              icon: LucideIcons.lock,
            ),
          ],
        ),
        SizedBox(height: tokens.space6),
        _SettingRow(
          tokens: tokens,
          title: '设备',
          description: config?.deviceName ?? '—',
        ),
        _SettingRow(
          tokens: tokens,
          title: '服务器',
          description: config?.serverUrl ?? '—',
        ),
        _SettingRow(
          tokens: tokens,
          title: '退出登录',
          description: '删除这台电脑上保存的登录状态与密钥。已备份的文件不受影响。',
          control: TokensButton(
            label: '退出登录',
            variant: TokensButtonVariant.ghost,
            onPressed: auth.busy ? null : onLogout,
          ),
        ),
      ],
    );
  }
}

class _AboutPanel extends StatelessWidget {
  const _AboutPanel({
    required this.tokens,
    required this.config,
    required this.onOpenSecurity,
    required this.onOpenLicenses,
    required this.onDownload,
  });

  final DesktopTokens tokens;
  final AppConfig? config;
  final VoidCallback onOpenSecurity;
  final VoidCallback onOpenLicenses;
  final VoidCallback onDownload;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _PanelTitle(tokens: tokens, text: '关于'),
        SizedBox(height: tokens.space5),
        Row(
          children: [
            BrandLogo(size: DesktopMetrics.logoSizeLarge),
            SizedBox(width: 14),
            Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  '加密备份',
                  style: TextStyle(
                    color: tokens.ink900,
                    fontSize: tokens.textLg,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                SizedBox(height: 2),
                Text(
                  '端对端加密的文件备份',
                  style: TextStyle(
                    color: tokens.ink500,
                    fontSize: tokens.textSm,
                  ),
                ),
              ],
            ),
          ],
        ),
        SizedBox(height: tokens.space4),
        _DescriptionRow(tokens: tokens, label: '版本', value: appVersion),
        _DescriptionRow(
          tokens: tokens,
          label: '提交',
          value: gitCommit,
          mono: true,
        ),
        _DescriptionRow(
          tokens: tokens,
          label: '服务器',
          value: config?.serverUrl ?? '—',
        ),
        SizedBox(height: tokens.space5),
        Wrap(
          spacing: tokens.space2,
          runSpacing: tokens.space2,
          children: [
            TokensButton(
              label: '下载最新版本',
              icon: LucideIcons.externalLink,
              onPressed: releasesUrl.isEmpty ? null : onDownload,
            ),
            TokensButton(
              label: '安全与隐私说明',
              icon: LucideIcons.shield,
              variant: TokensButtonVariant.ghost,
              onPressed: onOpenSecurity,
            ),
            TokensButton(
              label: '开源许可',
              variant: TokensButtonVariant.ghost,
              onPressed: onOpenLicenses,
            ),
          ],
        ),
      ],
    );
  }
}

class _PanelTitle extends StatelessWidget {
  const _PanelTitle({required this.tokens, required this.text});

  final DesktopTokens tokens;
  final String text;

  @override
  Widget build(BuildContext context) {
    return Text(
      text,
      style: TextStyle(
        color: tokens.ink900,
        fontSize: tokens.textXl,
        fontWeight: FontWeight.w600,
      ),
    );
  }
}

class _GroupTitle extends StatelessWidget {
  const _GroupTitle({required this.tokens, required this.text});

  final DesktopTokens tokens;
  final String text;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: 20, bottom: 4),
      child: Text(
        text,
        style: TextStyle(
          color: tokens.ink500,
          fontSize: tokens.textXs,
          fontWeight: FontWeight.w600,
        ),
      ),
    );
  }
}

class _SettingRow extends StatelessWidget {
  const _SettingRow({
    required this.tokens,
    required this.title,
    required this.description,
    this.control,
  });

  final DesktopTokens tokens;
  final String title;
  final String description;
  final Widget? control;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(vertical: 14),
      decoration: BoxDecoration(
        border: Border(bottom: BorderSide(color: tokens.line)),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  title,
                  style: TextStyle(
                    color: tokens.ink900,
                    fontSize: tokens.textSm,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                const SizedBox(height: 3),
                Text(
                  description,
                  style: TextStyle(
                    color: tokens.ink500,
                    fontSize: tokens.textXs,
                    height: 1.5,
                  ),
                ),
              ],
            ),
          ),
          if (control != null) ...[
            const SizedBox(width: 16),
            Padding(padding: const EdgeInsets.only(top: 1), child: control!),
          ],
        ],
      ),
    );
  }
}

class _SelectPlaceholder extends StatelessWidget {
  const _SelectPlaceholder({required this.label});

  final String label;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Opacity(
      opacity: 0.55,
      child: Container(
        height: DesktopMetrics.buttonHeight,
        padding: const EdgeInsets.symmetric(horizontal: 10),
        decoration: BoxDecoration(
          border: Border.all(color: tokens.lineStrong),
          borderRadius: BorderRadius.circular(tokens.radiusControl),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(
              label,
              style: TextStyle(color: tokens.ink900, fontSize: tokens.textSm),
            ),
            const SizedBox(width: 8),
            Icon(LucideIcons.chevronDown, size: 14, color: tokens.ink500),
          ],
        ),
      ),
    );
  }
}

class _DescriptionRow extends StatelessWidget {
  const _DescriptionRow({
    required this.tokens,
    required this.label,
    required this.value,
    this.mono = false,
  });

  final DesktopTokens tokens;
  final String label;
  final String value;
  final bool mono;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.only(top: 10),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 96,
            child: Text(
              label,
              style: TextStyle(color: tokens.ink500, fontSize: tokens.textSm),
            ),
          ),
          Expanded(
            child: Text(
              value,
              style: TextStyle(
                color: tokens.ink900,
                fontSize: mono ? tokens.textXs : tokens.textSm,
                fontFamily: mono ? DesktopMetrics.monoFamily : null,
                fontFamilyFallback: mono ? DesktopMetrics.monoFallback : null,
              ),
            ),
          ),
        ],
      ),
    );
  }
}
