import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../common/dialogs/keep_unlocked_dialog.dart';
import '../../common/state/auth_controller.dart';
import '../../common/widgets/controls.dart';
import '../../common/widgets/window_frame.dart';
import '../../core/errors/bridge_error_text.dart';
import '../../core/format/format.dart';
import '../../core/platform/file_picker.dart';
import '../../core/providers/app_providers.dart';
import '../../core/rust/models.dart';
import '../../core/theme/design_tokens.dart';
import '../onboarding/standard_folders.dart';
import '../settings/local_settings.dart';
import '../startup/startup_service.dart';

/// 首次引导（onboarding.html 的 folders / background 状态）。
///
/// 登录成功后第一次进入时显示，可跳过；完成标记保存在应用数据目录的
/// `desktop-settings.json`（不含任何密钥 / 令牌）。
class OnboardingPage extends ConsumerStatefulWidget {
  const OnboardingPage({super.key});

  @override
  ConsumerState<OnboardingPage> createState() => _OnboardingPageState();
}

class _OnboardingPageState extends ConsumerState<OnboardingPage> {
  List<StandardFolder> _folders = const <StandardFolder>[];
  final Map<String, BackupPreflightDto?> _preflight =
      <String, BackupPreflightDto?>{};
  Set<String> _selected = <String>{};
  bool _loading = true;
  bool _stepTwo = false;
  bool _startup = false;
  bool _keepUnlocked = false;
  bool _busy = false;
  String? _error;

  @override
  void initState() {
    super.initState();
    Future<void>.microtask(_load);
  }

  Future<void> _load() async {
    try {
      final folders = await ref.read(standardFoldersProvider.future);
      if (!mounted) {
        return;
      }
      setState(() {
        _folders = folders;
        _selected = <String>{
          for (final folder in folders)
            if (folder.kind == StandardFolderKind.desktop ||
                folder.kind == StandardFolderKind.documents)
              folder.path,
        };
        _loading = false;
      });
      for (final folder in folders) {
        await _measure(folder);
      }
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _loading = false;
        _error = bridgeErrorText(error);
      });
    }
  }

  Future<void> _measure(StandardFolder folder) async {
    BackupPreflightDto? preflight;
    try {
      preflight = await ref
          .read(nativeBridgeProvider)
          .backupPreflight(
            BackupFolderDraftDto(
              localPath: folder.path,
              excludeGlobs: const <String>[],
              excludeSubfolders: const <String>[],
            ),
          );
    } catch (_) {
      // 单个文件夹统计失败不影响其他条目。
    }
    if (!mounted) {
      return;
    }
    setState(() => _preflight[folder.path] = preflight);
  }

  Future<void> _addOtherFolder() async {
    final path = await ref.read(filePickerProvider).pickDirectory();
    if (path == null || !mounted) {
      return;
    }
    final name = baseName(path);
    if (_folders.any((folder) => folder.path == path)) {
      return;
    }
    final folder = StandardFolder(
      name: name,
      path: path,
      kind: StandardFolderKind.desktop,
    );
    setState(() {
      _folders = <StandardFolder>[..._folders, folder];
      _selected = <String>{..._selected, path};
    });
    await _measure(folder);
  }

  List<StandardFolder> get _selectedFolders => <StandardFolder>[
    for (final folder in _folders)
      if (_selected.contains(folder.path)) folder,
  ];

  int? get _selectedBytes {
    var total = 0;
    for (final folder in _selectedFolders) {
      final preflight = _preflight[folder.path];
      if (preflight == null) {
        return null;
      }
      total += preflight.uploadBytes;
    }
    return total;
  }

  int? get _remainingQuota {
    int? remaining;
    for (final folder in _selectedFolders) {
      final value = _preflight[folder.path]?.remainingQuotaBytes;
      if (value != null && (remaining == null || value < remaining)) {
        remaining = value;
      }
    }
    return remaining;
  }

  Future<void> _finish({required bool skipped}) async {
    if (_busy) {
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      if (!skipped) {
        final bridge = ref.read(nativeBridgeProvider);
        for (final folder in _selectedFolders) {
          await bridge.backupAddFolder(
            BackupFolderDraftDto(
              localPath: folder.path,
              excludeGlobs: const <String>[],
              excludeSubfolders: const <String>[],
            ),
          );
        }
        if (_startup) {
          await ref.read(startupServiceProvider).setEnabled(true);
        }
      }
      await ref.read(localSettingsProvider.notifier).markOnboardingCompleted();
      if (!mounted) {
        return;
      }
      context.go('/files');
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _busy = false;
        _error = bridgeErrorText(error);
      });
    }
  }

  Future<void> _toggleKeepUnlocked(bool enabled) async {
    if (!enabled) {
      await ref.read(authControllerProvider.notifier).disableKeepUnlocked();
      if (mounted) {
        setState(() => _keepUnlocked = false);
      }
      return;
    }
    final confirmed = await showKeepUnlockedDialog(context);
    if (confirmed && mounted) {
      setState(() => _keepUnlocked = true);
    }
  }

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final deviceName = ref.watch(appConfigProvider).value?.deviceName ?? '';
    final folderPath = <String>[
      '全部文件',
      '电脑备份',
      if (deviceName.isNotEmpty) deviceName,
    ].join(' / ');
    return AppWindowScaffold(
      width: DesktopMetrics.onboardingWindowWidth,
      height: DesktopMetrics.onboardingWindowHeight,
      title: '欢迎使用加密备份',
      child: Column(
        children: [
          Expanded(
            child: SingleChildScrollView(
              padding: const EdgeInsets.fromLTRB(48, 32, 48, 0),
              child: _stepTwo
                  ? _backgroundStep(tokens)
                  : _foldersStep(tokens, folderPath),
            ),
          ),
          _footer(tokens),
        ],
      ),
    );
  }

  Widget _foldersStep(DesktopTokens tokens, String destination) {
    final selectedBytes = _selectedBytes;
    final remaining = _remainingQuota;
    final totalText = selectedBytes == null
        ? '计算中…'
        : formatBytes(selectedBytes);
    final remainingText = remaining == null ? '—' : formatBytes(remaining);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _Steps(tokens: tokens, current: 1),
        const SizedBox(height: 20),
        Text(
          '选择要自动备份的文件夹',
          style: TextStyle(
            color: tokens.ink900,
            fontSize: tokens.text2xl,
            fontWeight: FontWeight.w600,
          ),
        ),
        const SizedBox(height: 6),
        Text(
          '这些文件夹里新增和修改的文件会在后台自动加密上传。之后随时可以在“备份文件夹”中修改。',
          style: TextStyle(
            color: tokens.ink500,
            fontSize: tokens.textSm,
            height: 1.5,
          ),
        ),
        const SizedBox(height: 22),
        if (_loading)
          const Padding(
            padding: EdgeInsets.symmetric(vertical: 40),
            child: Center(child: CircularProgressIndicator(strokeWidth: 2)),
          )
        else
          for (var index = 0; index < _folders.length; index += 1) ...[
            if (index > 0) const SizedBox(height: 10),
            _FolderChoice(
              key: ValueKey<String>(
                'onboarding-folder-${_folders[index].name}',
              ),
              tokens: tokens,
              folder: _folders[index],
              preflight: _preflight[_folders[index].path],
              selected: _selected.contains(_folders[index].path),
              onChanged: _busy
                  ? null
                  : (value) {
                      setState(() {
                        if (value) {
                          _selected = <String>{
                            ..._selected,
                            _folders[index].path,
                          };
                        } else {
                          _selected = <String>{..._selected}
                            ..remove(_folders[index].path);
                        }
                      });
                    },
            ),
          ],
        const SizedBox(height: 10),
        InkWell(
          key: const Key('onboarding-add-folder'),
          onTap: _busy ? null : _addOtherFolder,
          borderRadius: BorderRadius.circular(tokens.radiusCard),
          child: Container(
            height: 52,
            padding: const EdgeInsets.symmetric(horizontal: 16),
            decoration: BoxDecoration(
              border: Border.all(color: tokens.lineStrong),
              borderRadius: BorderRadius.circular(tokens.radiusCard),
            ),
            child: Row(
              children: [
                Icon(LucideIcons.plus, size: 16, color: tokens.brand700),
                const SizedBox(width: 10),
                Text(
                  '添加其他文件夹…',
                  style: TextStyle(
                    color: tokens.brand700,
                    fontSize: tokens.textSm,
                    fontWeight: FontWeight.w500,
                  ),
                ),
              ],
            ),
          ),
        ),
        const SizedBox(height: 18),
        TokensNote(
          text: '将备份到 $destination。已选 $totalText，剩余云端空间 $remainingText。',
          icon: LucideIcons.cloud,
        ),
      ],
    );
  }

  Widget _backgroundStep(DesktopTokens tokens) {
    final isMac = defaultTargetPlatform == TargetPlatform.macOS;
    final keychain = isMac ? 'macOS 钥匙串' : 'Windows 凭据保护';
    final tray = isMac ? '屏幕顶部的菜单栏' : '任务栏右下角的托盘';
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _Steps(tokens: tokens, current: 2),
        const SizedBox(height: 20),
        Text(
          '在后台继续备份',
          style: TextStyle(
            color: tokens.ink900,
            fontSize: tokens.text2xl,
            fontWeight: FontWeight.w600,
          ),
        ),
        const SizedBox(height: 6),
        Text(
          '关闭窗口后，加密备份会留在$tray里继续工作。',
          style: TextStyle(
            color: tokens.ink500,
            fontSize: tokens.textSm,
            height: 1.5,
          ),
        ),
        const SizedBox(height: 12),
        _BackgroundOption(
          tokens: tokens,
          title: '开机时自动启动',
          description: '只会启动程序；如果没有开启下面的选项，仍需要输入密码解锁后才会继续备份。',
          control: TokensToggle(
            key: const Key('onboarding-startup-toggle'),
            value: _startup,
            onChanged: _busy
                ? null
                : (value) => setState(() => _startup = value),
          ),
        ),
        _BackgroundOption(
          tokens: tokens,
          title: '在这台电脑上保持解锁',
          description:
              '重启后无需输入密码即可继续备份。你的密钥会交给系统保管（$keychain），'
              '只有这台电脑的当前系统账号能读取。',
          control: TokensToggle(
            key: const Key('onboarding-keep-unlocked-toggle'),
            value: _keepUnlocked,
            onChanged: _busy ? null : _toggleKeepUnlocked,
          ),
          detail: const TokensNote(
            text: '任何能登录这台电脑系统账号的人，都可能打开你的备份。只在你独自使用、并设有开机密码的电脑上开启。',
            icon: LucideIcons.alertTriangle,
            tone: NoteTone.warn,
          ),
        ),
      ],
    );
  }

  Widget _footer(DesktopTokens tokens) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(48, 16, 48, 24),
      child: Row(
        children: [
          if (_stepTwo)
            TokensButton(
              key: const Key('onboarding-back'),
              label: '上一步',
              icon: LucideIcons.chevronLeft,
              variant: TokensButtonVariant.ghost,
              onPressed: _busy ? null : () => setState(() => _stepTwo = false),
            )
          else
            TokensButton(
              key: const Key('onboarding-skip'),
              label: '稍后再说',
              variant: TokensButtonVariant.ghost,
              onPressed: _busy ? null : () => _finish(skipped: true),
            ),
          if (_error != null) ...[
            const SizedBox(width: 12),
            Expanded(
              child: Text(
                _error!,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  color: tokens.danger700,
                  fontSize: tokens.textXs,
                ),
              ),
            ),
          ] else
            const Spacer(),
          if (_stepTwo)
            TokensButton(
              key: const Key('onboarding-start'),
              label: _busy ? '正在开始…' : '开始备份',
              variant: TokensButtonVariant.primary,
              large: true,
              onPressed: _busy ? null : () => _finish(skipped: false),
            )
          else
            TokensButton(
              key: const Key('onboarding-next'),
              label: '下一步',
              trailingIcon: LucideIcons.arrowRight,
              variant: TokensButtonVariant.primary,
              large: true,
              onPressed: _busy ? null : () => setState(() => _stepTwo = true),
            ),
        ],
      ),
    );
  }
}

class _Steps extends StatelessWidget {
  const _Steps({required this.tokens, required this.current});

  final DesktopTokens tokens;
  final int current;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        for (var index = 1; index <= 2; index += 1) ...[
          if (index > 1) const SizedBox(width: 6),
          Container(
            width: 28,
            height: 4,
            decoration: BoxDecoration(
              color: index <= current ? tokens.brand600 : tokens.canvasStrong,
              borderRadius: BorderRadius.circular(2),
            ),
          ),
        ],
      ],
    );
  }
}

class _FolderChoice extends StatelessWidget {
  const _FolderChoice({
    required this.tokens,
    required this.folder,
    required this.preflight,
    required this.selected,
    required this.onChanged,
    super.key,
  });

  final DesktopTokens tokens;
  final StandardFolder folder;
  final BackupPreflightDto? preflight;
  final bool selected;
  final ValueChanged<bool>? onChanged;

  @override
  Widget build(BuildContext context) {
    final icon = switch (folder.kind) {
      StandardFolderKind.desktop => LucideIcons.monitor,
      StandardFolderKind.documents => LucideIcons.fileText,
      StandardFolderKind.pictures => LucideIcons.image,
    };
    final detail = preflight == null
        ? '计算中…'
        : '${formatCount(preflight!.fileCount)} 个文件 · ${formatBytes(preflight!.totalBytes)}';
    return InkWell(
      onTap: onChanged == null ? null : () => onChanged!(!selected),
      borderRadius: BorderRadius.circular(tokens.radiusCard),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
        decoration: BoxDecoration(
          color: selected ? tokens.brand50 : tokens.surface,
          border: Border.all(color: selected ? tokens.brand600 : tokens.line),
          borderRadius: BorderRadius.circular(tokens.radiusCard),
        ),
        child: Row(
          children: [
            Container(
              width: 18,
              height: 18,
              decoration: BoxDecoration(
                color: selected ? tokens.brand600 : tokens.surface,
                border: Border.all(
                  color: selected ? tokens.brand600 : tokens.lineStrong,
                  width: 1.5,
                ),
                borderRadius: BorderRadius.circular(5),
              ),
              child: selected
                  ? Icon(LucideIcons.check, size: 12, color: tokens.onBrand)
                  : null,
            ),
            const SizedBox(width: 14),
            Container(
              width: 40,
              height: 40,
              decoration: BoxDecoration(
                color: selected ? tokens.surface : tokens.canvas,
                borderRadius: BorderRadius.circular(10),
              ),
              child: Icon(
                icon,
                size: 20,
                color: selected ? tokens.brand600 : tokens.ink600,
              ),
            ),
            const SizedBox(width: 14),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    folder.name,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      color: tokens.ink900,
                      fontSize: tokens.textSm,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                  const SizedBox(height: 2),
                  Text(
                    folder.path,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      color: tokens.ink500,
                      fontSize: tokens.textXs,
                      fontFamily: DesktopMetrics.monoFamily,
                      fontFamilyFallback: DesktopMetrics.monoFallback,
                    ),
                  ),
                ],
              ),
            ),
            const SizedBox(width: 12),
            SizedBox(
              width: 140,
              child: Text(
                detail,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                textAlign: TextAlign.right,
                style: TextStyle(color: tokens.ink500, fontSize: tokens.textXs),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _BackgroundOption extends StatelessWidget {
  const _BackgroundOption({
    required this.tokens,
    required this.title,
    required this.description,
    required this.control,
    this.detail,
  });

  final DesktopTokens tokens;
  final String title;
  final String description;
  final Widget control;
  final Widget? detail;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.symmetric(vertical: 16),
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
                if (detail != null) ...[const SizedBox(height: 10), detail!],
              ],
            ),
          ),
          const SizedBox(width: 16),
          Padding(padding: const EdgeInsets.only(top: 1), child: control),
        ],
      ),
    );
  }
}
