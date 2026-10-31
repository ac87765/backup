import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/errors/bridge_error_text.dart';
import '../../core/theme/design_tokens.dart';
import '../state/auth_controller.dart';
import '../widgets/app_dialog.dart';
import '../widgets/controls.dart';

/// 开启“保持解锁”前的风险确认（settings.html 的 confirm 状态）。
///
/// 设置页与首次引导共用；成功后保持解锁已开启。
Future<bool> showKeepUnlockedDialog(BuildContext context) async {
  final result = await showTokensDialog<bool>(
    context: context,
    builder: (dialogContext) => const KeepUnlockedDialog(),
  );
  return result ?? false;
}

class KeepUnlockedDialog extends ConsumerStatefulWidget {
  const KeepUnlockedDialog({super.key});

  @override
  ConsumerState<KeepUnlockedDialog> createState() => _KeepUnlockedDialogState();
}

class _KeepUnlockedDialogState extends ConsumerState<KeepUnlockedDialog> {
  final TextEditingController _password = TextEditingController();
  String? _error;
  bool _busy = false;

  @override
  void dispose() {
    _password.dispose();
    super.dispose();
  }

  Future<void> _confirm() async {
    if (_password.text.isEmpty) {
      setState(() => _error = '请输入密码');
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await ref
          .read(authControllerProvider.notifier)
          .enableKeepUnlocked(_password.text);
      if (!mounted) {
        return;
      }
      Navigator.of(context).pop(true);
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

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return TokensDialogShell(
      title: '开启“保持解锁”？',
      width: DesktopMetrics.dialogWidthSmall,
      headerIcon: LucideIcons.alertTriangle,
      headerIconColor: tokens.warning600,
      headerIconBackground: tokens.warning50,
      onClose: _busy ? () {} : () => Navigator.of(context).pop(false),
      body: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            '开启后，任何能登录这台电脑系统账号的人都可以打开加密备份并查看、下载你的文件。',
            style: TextStyle(
              color: tokens.ink900,
              fontSize: tokens.textSm,
              height: 1.5,
            ),
          ),
          const SizedBox(height: 8),
          Text(
            '只建议在你独自使用、并设有开机密码的电脑上开启。退出登录或修改密码时会自动关闭。',
            style: TextStyle(
              color: tokens.ink500,
              fontSize: tokens.textXs,
              height: 1.5,
            ),
          ),
          const SizedBox(height: 18),
          Text(
            '输入密码以确认',
            style: TextStyle(
              color: tokens.ink900,
              fontSize: tokens.textSm,
              fontWeight: FontWeight.w600,
            ),
          ),
          const SizedBox(height: 6),
          AppTextField(
            key: const Key('keep-unlocked-password'),
            controller: _password,
            obscure: true,
            enabled: !_busy,
            error: _error != null,
            autofocus: true,
            autofillHints: const <String>[AutofillHints.password],
            onSubmitted: (_) => _confirm(),
          ),
          if (_error != null) ...[
            const SizedBox(height: 6),
            Text(
              _error!,
              style: TextStyle(
                color: tokens.danger700,
                fontSize: tokens.textXs,
              ),
            ),
          ],
        ],
      ),
      actions: [
        TokensButton(
          key: const Key('keep-unlocked-cancel'),
          label: '取消',
          onPressed: _busy ? null : () => Navigator.of(context).pop(false),
        ),
        TokensButton(
          key: const Key('keep-unlocked-confirm'),
          label: _busy ? '开启中…' : '开启',
          variant: TokensButtonVariant.primary,
          onPressed: _busy ? null : _confirm,
        ),
      ],
    );
  }
}
