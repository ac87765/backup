import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../common/state/transfer_controller.dart';
import '../../common/widgets/controls.dart';
import '../../core/clock.dart';
import '../../core/format/format.dart';
import '../../core/platform/file_picker.dart';
import '../../core/providers/app_providers.dart';
import '../../core/rust/models.dart';
import '../../core/theme/design_tokens.dart';
import '../dialogs/wechat_note_dialog.dart';
import '../dialogs/wechat_restore_dialog.dart';
import '../state/wechat_controller.dart';
import '../state/wechat_status.dart';
import '../widgets/update_banner.dart';

/// “微信备份”页面（design/desktop/wechat.html 的五个状态）。
///
/// 页面只展示发现结果和本地/云端状态；不读取、不解密微信内容。
class WechatBackupPage extends ConsumerStatefulWidget {
  const WechatBackupPage({super.key});

  @override
  ConsumerState<WechatBackupPage> createState() => _WechatBackupPageState();
}

class _WechatBackupPageState extends ConsumerState<WechatBackupPage> {
  final Set<String> _expanded = <String>{};
  bool _expandedInitialized = false;

  bool _isExpanded(String accountName) => _expanded.contains(accountName);

  void _toggleExpanded(String accountName) {
    setState(() {
      if (!_expanded.remove(accountName)) {
        _expanded.add(accountName);
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final state = ref.watch(wechatControllerProvider);
    if (!_expandedInitialized && state.accounts.isNotEmpty) {
      _expandedInitialized = true;
      _expanded.add(state.accounts.first.accountName);
    }
    return Column(
      children: [
        _Toolbar(state: state),
        const UpdateBanner(),
        Expanded(child: _Content(state: state)),
      ],
    );
  }

  Future<void> _pickDirectory() async {
    final path = await ref.read(filePickerProvider).pickDirectory();
    if (path == null || !mounted) {
      return;
    }
    await ref.read(wechatControllerProvider.notifier).addManualDirectory(path);
  }

  Future<void> _restore(WechatPageState state) async {
    if (state.enabledAccounts.isEmpty) {
      return;
    }
    final started = await showWechatRestoreDialog(
      context: context,
      account: state.enabledAccounts.first,
    );
    if (started && mounted) {
      ref.read(transferPanelOpenProvider.notifier).open();
      _showMessage('已开始恢复；请保持微信关闭直到完成');
    }
  }

  void _showMessage(String message) {
    ScaffoldMessenger.maybeOf(context)?.showSnackBar(
      SnackBar(
        content: Text(message),
        behavior: SnackBarBehavior.floating,
        width: 360,
      ),
    );
  }
}

class _Toolbar extends ConsumerWidget {
  const _Toolbar({required this.state});

  final WechatPageState state;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = DesktopTokens.of(context);
    final controller = ref.read(wechatControllerProvider.notifier);
    final enabled = state.hasEnabledAccounts && state.accounts.isNotEmpty;
    final paused = state.globalPaused;
    return Container(
      height: DesktopMetrics.toolbarHeight,
      padding: const EdgeInsets.symmetric(horizontal: 20),
      decoration: BoxDecoration(
        border: Border(bottom: BorderSide(color: tokens.line)),
      ),
      child: Row(
        children: [
          Text(
            '微信备份',
            style: TextStyle(
              color: tokens.ink900,
              fontSize: tokens.textXl,
              fontWeight: FontWeight.w600,
            ),
          ),
          const Spacer(),
          if (enabled) ...[
            TokensButton(
              key: const Key('wechat-pause'),
              label: paused ? '恢复' : '暂停',
              icon: paused ? LucideIcons.play : LucideIcons.pause,
              onPressed: state.busy
                  ? null
                  : (paused ? controller.resume : controller.pause),
            ),
            const SizedBox(width: 8),
            TokensButton(
              key: const Key('wechat-backup-now'),
              label: '立即备份',
              icon: LucideIcons.refreshCw,
              onPressed: state.busy ? null : controller.scanNow,
            ),
            const SizedBox(width: 8),
            TokensButton(
              key: const Key('wechat-restore'),
              label: '恢复到微信…',
              icon: LucideIcons.rotateCcw,
              onPressed: () async {
                final page = context
                    .findAncestorStateOfType<_WechatBackupPageState>();
                await page?._restore(state);
              },
            ),
          ],
        ],
      ),
    );
  }
}

class _Content extends ConsumerWidget {
  const _Content({required this.state});

  final WechatPageState state;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = DesktopTokens.of(context);
    return SingleChildScrollView(
      padding: const EdgeInsets.fromLTRB(28, 24, 28, 24),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const TokensNote(
            key: Key('wechat-privacy-note'),
            text: '把微信的聊天文件、图片视频和聊天记录加密备份到云端，换电脑或重装后可以恢复。我们不会读取或解密你的聊天内容，聊天记录只能恢复到微信中查看。',
            icon: LucideIcons.shieldCheck,
            tone: NoteTone.info,
          ),
          if (state.error != null) ...[
            const SizedBox(height: 12),
            Text(
              state.error!,
              key: const Key('wechat-error'),
              style: TextStyle(
                color: tokens.danger700,
                fontSize: tokens.textXs,
              ),
            ),
          ],
          const SizedBox(height: 16),
          if (state.loading && state.accounts.isEmpty)
            const Padding(
              padding: EdgeInsets.symmetric(vertical: 80),
              child: Center(child: CircularProgressIndicator(strokeWidth: 2)),
            )
          else if (state.accounts.isEmpty)
            _NotFoundState(
              busy: state.busy,
              onPickDirectory: () async {
                final page = context
                    .findAncestorStateOfType<_WechatBackupPageState>();
                await page?._pickDirectory();
              },
            )
          else if (!state.hasEnabledAccounts)
            _SetupCard(
              state: state,
              onPickDirectory: () async {
                final page = context
                    .findAncestorStateOfType<_WechatBackupPageState>();
                await page?._pickDirectory();
              },
            )
          else ...[
            for (var index = 0; index < state.accounts.length; index += 1) ...[
              if (index > 0) SizedBox(height: tokens.space3),
              _AccountCard(
                account: state.accounts[index],
                status: state.statusFor(state.accounts[index]),
                expanded: _expandedOf(context, state.accounts[index]),
                primary: index == 0,
                globalPaused: state.globalPaused,
                busy: state.busy,
                now: ref.read(clockProvider)(),
                deviceName:
                    ref.watch(appConfigProvider).value?.deviceName ?? '',
                onToggle: () => _toggleExpanded(context, state.accounts[index]),
                onSetNote: () async {
                  final account = state.accounts[index];
                  final result = await showWechatNoteDialog(
                    context: context,
                    accountName: account.accountName,
                    initialNote: account.note,
                  );
                  if (result == null || !context.mounted) {
                    return;
                  }
                  final controller = ref.read(
                    wechatControllerProvider.notifier,
                  );
                  switch (result) {
                    case WechatNoteSave(:final note):
                      await controller.setNote(account, note);
                      break;
                    case WechatNoteClear():
                      await controller.clearNote(account);
                      break;
                  }
                },
              ),
            ],
          ],
        ],
      ),
    );
  }

  bool _expandedOf(BuildContext context, WechatAccountDto account) {
    final page = context.findAncestorStateOfType<_WechatBackupPageState>();
    return page?._isExpanded(account.accountName) ?? false;
  }

  void _toggleExpanded(BuildContext context, WechatAccountDto account) {
    context.findAncestorStateOfType<_WechatBackupPageState>()?._toggleExpanded(
      account.accountName,
    );
  }
}

class _NotFoundState extends StatelessWidget {
  const _NotFoundState({required this.busy, required this.onPickDirectory});

  final bool busy;
  final Future<void> Function() onPickDirectory;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Container(
      padding: const EdgeInsets.symmetric(vertical: 64, horizontal: 24),
      decoration: BoxDecoration(
        color: tokens.surface,
        border: Border.all(color: tokens.line),
        borderRadius: BorderRadius.circular(tokens.radiusCard),
      ),
      child: Column(
        children: [
          Container(
            width: 56,
            height: 56,
            decoration: BoxDecoration(
              color: tokens.canvasStrong,
              borderRadius: BorderRadius.circular(14),
            ),
            child: Icon(
              LucideIcons.messageCircle,
              size: 28,
              color: tokens.ink500,
            ),
          ),
          const SizedBox(height: 12),
          Text(
            '没有找到微信数据',
            style: TextStyle(
              color: tokens.ink900,
              fontSize: tokens.textLg,
              fontWeight: FontWeight.w600,
            ),
          ),
          const SizedBox(height: 6),
          ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 440),
            child: Text(
              '微信可能装在其他位置，或者还没有在这台电脑上登录过。可以在微信“设置 → 文件管理”中看到存储位置，然后手动选择。',
              textAlign: TextAlign.center,
              style: TextStyle(
                color: tokens.ink500,
                fontSize: tokens.textSm,
                height: 1.5,
              ),
            ),
          ),
          const SizedBox(height: 20),
          TokensButton(
            key: const Key('wechat-pick-directory'),
            label: '手动选择微信数据文件夹…',
            icon: LucideIcons.folder,
            variant: TokensButtonVariant.primary,
            busy: busy,
            onPressed: busy ? null : onPickDirectory,
          ),
        ],
      ),
    );
  }
}

class _SetupCard extends StatelessWidget {
  const _SetupCard({required this.state, required this.onPickDirectory});

  final WechatPageState state;
  final Future<void> Function() onPickDirectory;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final ref = ProviderScope.containerOf(context);
    final notifier = ref.read(wechatControllerProvider.notifier);
    final preflight = state.setupPreflight;
    final quota = state.quotaExceeded;
    final multipleRoots =
        state.accounts.map((account) => account.dataRoot).toSet().length > 1;
    final first = state.accounts.first;
    final categories = <WechatCategoryDto, _AggregateCategory>{
      for (final category in WechatCategoryDto.values)
        category: _aggregateCategory(state.accounts, category),
    };
    return Container(
      key: const Key('wechat-setup-card'),
      decoration: BoxDecoration(
        color: tokens.surface,
        border: Border.all(color: tokens.line),
        borderRadius: BorderRadius.circular(tokens.radiusCard),
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(18, 16, 18, 16),
            child: Row(
              children: [
                Container(
                  width: 40,
                  height: 40,
                  decoration: BoxDecoration(
                    color: tokens.success50,
                    borderRadius: BorderRadius.circular(10),
                  ),
                  child: Icon(
                    LucideIcons.messageCircle,
                    size: 20,
                    color: tokens.success600,
                  ),
                ),
                const SizedBox(width: 14),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        '找到 ${state.accounts.length} 个微信账号',
                        style: TextStyle(
                          color: tokens.ink900,
                          fontSize: tokens.textMd,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                      const SizedBox(height: 4),
                      Wrap(
                        spacing: 6,
                        runSpacing: 4,
                        crossAxisAlignment: WrapCrossAlignment.center,
                        children: [
                          Text(
                            wechatVersionLabel(first.version),
                            style: TextStyle(
                              color: tokens.ink500,
                              fontSize: tokens.textXs,
                            ),
                          ),
                          Text(
                            '·',
                            style: TextStyle(
                              color: tokens.ink500,
                              fontSize: tokens.textXs,
                            ),
                          ),
                          Text(
                            multipleRoots ? '多个微信数据位置' : first.dataRoot,
                            style: TextStyle(
                              color: tokens.ink500,
                              fontSize: tokens.textXs,
                              fontFamily: DesktopMetrics.monoFamily,
                              fontFamilyFallback: DesktopMetrics.monoFallback,
                            ),
                          ),
                          InkWell(
                            key: const Key('wechat-pick-directory-link'),
                            onTap: state.busy ? null : onPickDirectory,
                            child: Text(
                              '不是这里？手动选择…',
                              style: TextStyle(
                                color: tokens.brand700,
                                fontSize: tokens.textXs,
                              ),
                            ),
                          ),
                        ],
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
          Container(
            decoration: BoxDecoration(
              border: Border(top: BorderSide(color: tokens.line)),
            ),
            child: Column(
              children: [
                for (final category in WechatCategoryDto.values)
                  _CategoryRow(
                    key: ValueKey<String>(
                      'wechat-setup-category-${category.name}',
                    ),
                    tokens: tokens,
                    category: category,
                    selected: state.setupCategories.contains(category),
                    enabled:
                        categories[category]!.exists &&
                        !state.busy &&
                        !state.preflightLoading,
                    description: wechatCategoryDescription(
                      category,
                      setup: true,
                    ),
                    sizeText: categories[category]!.exists
                        ? formatBytes(categories[category]!.bytes)
                        : '—',
                    statusText:
                        quota &&
                            category == WechatCategoryDto.imagesAndVideos &&
                            state.setupCategories.contains(category)
                        ? '建议先取消'
                        : null,
                    statusTone: quota
                        ? WechatStatusTone.warn
                        : WechatStatusTone.muted,
                    onTap: () => notifier.toggleSetupCategory(category),
                    showBorder: category != WechatCategoryDto.values.last,
                  ),
              ],
            ),
          ),
          Container(
            padding: const EdgeInsets.fromLTRB(18, 16, 18, 16),
            decoration: BoxDecoration(
              border: Border(top: BorderSide(color: tokens.line)),
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                _SetupSummary(
                  tokens: tokens,
                  preflight: preflight,
                  quota: quota,
                  loading: state.preflightLoading,
                ),
                if (quota) ...[
                  const SizedBox(height: 12),
                  const TokensNote(
                    key: Key('wechat-quota-warning'),
                    text: '空间不够备份全部内容。可以先取消“图片与视频”，聊天文件和聊天记录会照常备份。',
                    icon: LucideIcons.alertTriangle,
                    tone: NoteTone.warn,
                  ),
                ],
                if (state.preflightError != null) ...[
                  const SizedBox(height: 8),
                  Text(
                    state.preflightError!,
                    style: TextStyle(
                      color: tokens.danger700,
                      fontSize: tokens.textXs,
                    ),
                  ),
                ],
                const SizedBox(height: 12),
                Row(
                  mainAxisAlignment: MainAxisAlignment.end,
                  children: [
                    TokensButton(
                      key: const Key('wechat-start-backup'),
                      label: '开启微信备份',
                      icon: LucideIcons.shieldCheck,
                      variant: TokensButtonVariant.primary,
                      busy: state.busy,
                      onPressed:
                          state.busy ||
                              state.preflightLoading ||
                              quota ||
                              state.setupCategories.isEmpty
                          ? null
                          : notifier.startSetup,
                    ),
                  ],
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _AggregateCategory {
  const _AggregateCategory({required this.exists, required this.bytes});

  final bool exists;
  final int bytes;
}

_AggregateCategory _aggregateCategory(
  List<WechatAccountDto> accounts,
  WechatCategoryDto category,
) {
  var exists = false;
  var bytes = 0;
  for (final account in accounts) {
    final info = wechatCategoryInfo(account, category);
    if (info == null) {
      continue;
    }
    exists = exists || info.exists;
    bytes += info.sizeBytes;
  }
  return _AggregateCategory(exists: exists, bytes: bytes);
}

class _SetupSummary extends StatelessWidget {
  const _SetupSummary({
    required this.tokens,
    required this.preflight,
    required this.quota,
    required this.loading,
  });

  final DesktopTokens tokens;
  final WechatSetupPreflight? preflight;
  final bool quota;
  final bool loading;

  @override
  Widget build(BuildContext context) {
    final value = preflight;
    final upload = value?.uploadBytes;
    final remaining = value?.remainingQuotaBytes;
    final thirdValue = quota
        ? (value?.uploadBytesWithoutImagesAndVideos)
        : null;
    final estimate = upload == null ? null : _formatUploadEstimate(upload);
    return Container(
      key: const Key('wechat-setup-summary'),
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
      decoration: BoxDecoration(
        color: quota ? tokens.danger50 : tokens.canvas,
        borderRadius: BorderRadius.circular(tokens.radiusControl),
      ),
      child: Row(
        children: [
          Expanded(
            child: _SummaryCell(
              tokens: tokens,
              value: loading && value == null ? '正在统计…' : formatBytes(upload),
              label: '首次需要上传',
              valueColor: quota ? tokens.danger700 : null,
            ),
          ),
          Expanded(
            child: _SummaryCell(
              tokens: tokens,
              value: remaining == null ? '—' : formatBytes(remaining),
              label: '剩余云端空间',
            ),
          ),
          Expanded(
            child: _SummaryCell(
              tokens: tokens,
              value: quota
                  ? formatBytes(thirdValue)
                  : (loading && value == null ? '正在统计…' : (estimate ?? '—')),
              label: quota ? '取消“图片与视频”后需要上传' : '首次备份预计（之后只传变化部分）',
            ),
          ),
        ],
      ),
    );
  }
}

String _formatUploadEstimate(int uploadBytes) {
  if (uploadBytes <= 0) {
    return '—';
  }
  // 本机没有“真实带宽”数据，给一个保守的 1 MB/s 估算；文案明确是“约”。
  const bytesPerSecond = 1_000_000;
  final seconds = (uploadBytes / bytesPerSecond).ceil();
  return formatEta(Duration(seconds: seconds));
}

class _SummaryCell extends StatelessWidget {
  const _SummaryCell({
    required this.tokens,
    required this.value,
    required this.label,
    this.valueColor,
  });

  final DesktopTokens tokens;
  final String value;
  final String label;
  final Color? valueColor;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          value,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: TextStyle(
            color: valueColor ?? tokens.ink900,
            fontSize: tokens.textLg,
            fontWeight: FontWeight.w600,
          ),
        ),
        const SizedBox(height: 2),
        Text(
          label,
          maxLines: 2,
          overflow: TextOverflow.ellipsis,
          style: TextStyle(color: tokens.ink500, fontSize: tokens.textXs),
        ),
      ],
    );
  }
}

class _AccountCard extends StatelessWidget {
  const _AccountCard({
    required this.account,
    required this.status,
    required this.expanded,
    required this.primary,
    required this.globalPaused,
    required this.busy,
    required this.now,
    required this.deviceName,
    required this.onToggle,
    required this.onSetNote,
  });

  final WechatAccountDto account;
  final WechatAccountBackupStatusDto? status;
  final bool expanded;
  final bool primary;
  final bool globalPaused;
  final bool busy;
  final DateTime now;
  final String deviceName;
  final VoidCallback onToggle;
  final Future<void> Function() onSetNote;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final statusView = wechatAccountStatusView(
      status,
      globalPaused: globalPaused,
    );
    final lastBackupAt = status?.lastBackupAt;
    final database = wechatCategoryStatus(status, WechatCategoryDto.databases);
    final displayName = account.note?.isNotEmpty == true
        ? account.note!
        : account.accountName;
    final cloudPath = <String>[
      '全部文件',
      '微信备份',
      if (deviceName.isNotEmpty) deviceName,
      account.accountName,
    ].join(' / ');
    return Container(
      key: ValueKey<String>('wechat-account-${account.accountName}'),
      decoration: BoxDecoration(
        color: tokens.surface,
        border: Border.all(color: tokens.line),
        borderRadius: BorderRadius.circular(tokens.radiusCard),
      ),
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(18, 16, 18, 16),
            child: Row(
              children: [
                Container(
                  width: 40,
                  height: 40,
                  decoration: BoxDecoration(
                    color: primary ? tokens.success50 : tokens.canvasStrong,
                    borderRadius: BorderRadius.circular(10),
                  ),
                  child: Icon(
                    LucideIcons.messageCircle,
                    size: 20,
                    color: primary ? tokens.success600 : tokens.ink500,
                  ),
                ),
                const SizedBox(width: 14),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        displayName,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          color: tokens.ink900,
                          fontSize: tokens.textMd,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                      const SizedBox(height: 4),
                      Wrap(
                        spacing: 6,
                        runSpacing: 2,
                        crossAxisAlignment: WrapCrossAlignment.center,
                        children: [
                          Text(
                            wechatVersionLabel(account.version),
                            style: TextStyle(
                              color: tokens.ink500,
                              fontSize: tokens.textXs,
                            ),
                          ),
                          Text(
                            '·',
                            style: TextStyle(
                              color: tokens.ink500,
                              fontSize: tokens.textXs,
                            ),
                          ),
                          Text(
                            formatBytes(account.totalSizeBytes),
                            style: TextStyle(
                              color: tokens.ink500,
                              fontSize: tokens.textXs,
                            ),
                          ),
                          Text(
                            '·',
                            style: TextStyle(
                              color: tokens.ink500,
                              fontSize: tokens.textXs,
                            ),
                          ),
                          Text(
                            account.isCurrentLoginCandidate
                                ? '可能是当前登录的账号'
                                : _lastModifiedLabel(
                                    account.lastModifiedNs,
                                    now,
                                  ),
                            style: TextStyle(
                              color: account.isCurrentLoginCandidate
                                  ? tokens.brand700
                                  : tokens.ink500,
                              fontSize: tokens.textXs,
                            ),
                          ),
                          InkWell(
                            key: ValueKey<String>(
                              'wechat-note-${account.accountName}',
                            ),
                            onTap: busy ? null : onSetNote,
                            child: Text(
                              '设置备注名',
                              style: TextStyle(
                                color: tokens.brand700,
                                fontSize: tokens.textXs,
                              ),
                            ),
                          ),
                        ],
                      ),
                    ],
                  ),
                ),
                const SizedBox(width: 12),
                _AccountStatusPill(view: statusView),
                const SizedBox(width: 12),
                Text(
                  formatBackupTime(lastBackupAt, now: now),
                  style: TextStyle(
                    color: tokens.ink500,
                    fontSize: tokens.textXs,
                  ),
                ),
                const SizedBox(width: 4),
                TokensIconButton(
                  key: ValueKey<String>('wechat-expand-${account.accountName}'),
                  icon: expanded
                      ? LucideIcons.chevronDown
                      : LucideIcons.chevronRight,
                  tooltip: expanded ? '收起' : '展开',
                  onPressed: onToggle,
                ),
              ],
            ),
          ),
          if (expanded) ...[
            Container(
              decoration: BoxDecoration(
                border: Border(top: BorderSide(color: tokens.line)),
              ),
              child: Column(
                children: [
                  for (final category in WechatCategoryDto.values)
                    _AccountCategoryRow(
                      account: account,
                      category: category,
                      status: status,
                      globalPaused: globalPaused,
                      busy: busy,
                      showBorder: category != WechatCategoryDto.values.last,
                    ),
                ],
              ),
            ),
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 12),
              decoration: BoxDecoration(
                color: tokens.canvas,
                border: Border(top: BorderSide(color: tokens.line)),
              ),
              child: Row(
                children: [
                  Icon(LucideIcons.cloud, size: 14, color: tokens.ink500),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      '备份到 $cloudPath',
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        color: tokens.ink500,
                        fontSize: tokens.textXs,
                      ),
                    ),
                  ),
                  const SizedBox(width: 12),
                  Text(
                    '上次数据库快照：${formatBackupTime(database?.lastBackupAt, now: now)}',
                    style: TextStyle(
                      color: tokens.ink500,
                      fontSize: tokens.textXs,
                    ),
                  ),
                ],
              ),
            ),
          ],
        ],
      ),
    );
  }

  String _lastModifiedLabel(int? nanoseconds, DateTime now) {
    if (nanoseconds == null || nanoseconds <= 0) {
      return '最近修改 —';
    }
    final at = DateTime.fromMillisecondsSinceEpoch(nanoseconds ~/ 1000000);
    return '最近修改 ${formatDateTime(at, now: now)}';
  }
}

class _AccountStatusPill extends StatelessWidget {
  const _AccountStatusPill({required this.view});

  final WechatStatusView view;

  @override
  Widget build(BuildContext context) {
    final (tone, icon) = switch (view.tone) {
      WechatStatusTone.ok => (PillTone.ok, LucideIcons.check),
      WechatStatusTone.busy => (PillTone.busy, LucideIcons.cloudUpload),
      WechatStatusTone.paused => (PillTone.paused, LucideIcons.clock),
      WechatStatusTone.warn => (PillTone.warn, LucideIcons.alertTriangle),
      WechatStatusTone.muted => (PillTone.paused, LucideIcons.messageCircle),
    };
    return TokensPill(label: view.label, tone: tone, icon: icon);
  }
}

class _AccountCategoryRow extends ConsumerWidget {
  const _AccountCategoryRow({
    required this.account,
    required this.category,
    required this.status,
    required this.globalPaused,
    required this.busy,
    required this.showBorder,
  });

  final WechatAccountDto account;
  final WechatCategoryDto category;
  final WechatAccountBackupStatusDto? status;
  final bool globalPaused;
  final bool busy;
  final bool showBorder;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = DesktopTokens.of(context);
    final info = wechatCategoryInfo(account, category);
    final categoryStatus = wechatCategoryStatus(status, category);
    var view = wechatCategoryStatusView(
      category: category,
      info: info,
      status: categoryStatus,
    );
    if (globalPaused && categoryStatus?.selected == true) {
      view = const WechatStatusView(
        label: '已暂停',
        tone: WechatStatusTone.paused,
      );
    }
    final selected = categoryStatus?.selected ?? false;
    final enabled = info?.exists == true && !busy;
    return _CategoryRow(
      key: ValueKey<String>(
        'wechat-category-${account.accountName}-${category.name}',
      ),
      tokens: tokens,
      category: category,
      selected: selected,
      enabled: enabled,
      description: wechatCategoryDescription(category),
      sizeText: info != null && info.exists ? formatBytes(info.sizeBytes) : '—',
      statusText: view.label,
      statusTone: view.tone,
      statusDetail: view.detail,
      onTap: () => ref
          .read(wechatControllerProvider.notifier)
          .setAccountCategory(account, category, !selected),
      showBorder: showBorder,
    );
  }
}

class _CategoryRow extends StatelessWidget {
  const _CategoryRow({
    required this.tokens,
    required this.category,
    required this.selected,
    required this.enabled,
    required this.description,
    required this.sizeText,
    required this.statusText,
    required this.statusTone,
    this.statusDetail,
    required this.onTap,
    required this.showBorder,
    super.key,
  });

  final DesktopTokens tokens;
  final WechatCategoryDto category;
  final bool selected;
  final bool enabled;
  final String description;
  final String sizeText;
  final String? statusText;
  final WechatStatusTone statusTone;
  final String? statusDetail;
  final VoidCallback onTap;
  final bool showBorder;

  @override
  Widget build(BuildContext context) {
    final (icon, color) = switch (category) {
      WechatCategoryDto.chatFiles => (LucideIcons.fileText, tokens.ink500),
      WechatCategoryDto.imagesAndVideos => (LucideIcons.image, tokens.ink500),
      WechatCategoryDto.databases => (LucideIcons.database, tokens.ink500),
      WechatCategoryDto.phoneMigration => (
        LucideIcons.smartphone,
        tokens.ink500,
      ),
    };
    final textColor = selected ? tokens.ink900 : tokens.ink400;
    final (statusColor, statusIcon) = switch (statusTone) {
      WechatStatusTone.ok => (tokens.success600, LucideIcons.checkCircle),
      WechatStatusTone.busy => (tokens.brand600, LucideIcons.cloudUpload),
      WechatStatusTone.paused => (tokens.warningTextStrong, LucideIcons.clock),
      WechatStatusTone.warn => (
        tokens.warningTextStrong,
        LucideIcons.alertTriangle,
      ),
      WechatStatusTone.muted => (tokens.ink500, null),
    };
    return InkWell(
      onTap: enabled ? onTap : null,
      child: Container(
        height: 52,
        padding: const EdgeInsets.symmetric(horizontal: 18),
        decoration: BoxDecoration(
          border: showBorder
              ? Border(bottom: BorderSide(color: tokens.line))
              : null,
        ),
        child: Row(
          children: [
            SizedBox(
              width: 22,
              child: Align(
                alignment: Alignment.centerLeft,
                child: TokensCheck(value: selected, enabled: enabled),
              ),
            ),
            SizedBox(width: 28, child: Icon(icon, size: 16, color: color)),
            Expanded(
              child: Column(
                mainAxisAlignment: MainAxisAlignment.center,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    wechatCategoryLabel(category),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      color: textColor,
                      fontSize: tokens.textSm,
                      fontWeight: FontWeight.w500,
                    ),
                  ),
                  const SizedBox(height: 2),
                  Text(
                    description,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      color: selected ? tokens.ink500 : tokens.ink400,
                      fontSize: tokens.textXs,
                    ),
                  ),
                ],
              ),
            ),
            const SizedBox(width: 10),
            SizedBox(
              width: 96,
              child: Text(
                sizeText,
                textAlign: TextAlign.right,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(color: textColor, fontSize: tokens.textSm),
              ),
            ),
            const SizedBox(width: 10),
            SizedBox(
              width: 220,
              child: Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  if (statusIcon != null) ...[
                    Icon(statusIcon, size: 14, color: statusColor),
                    const SizedBox(width: 6),
                  ],
                  Flexible(
                    child: Tooltip(
                      message: statusDetail ?? '',
                      child: Text(
                        statusText ?? '',
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        textAlign: TextAlign.right,
                        style: TextStyle(
                          color: statusColor,
                          fontSize: tokens.textXs,
                        ),
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
      ),
    );
  }
}
