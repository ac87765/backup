import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/platform/external_links.dart';
import '../../core/providers/app_providers.dart';
import '../../core/theme/design_tokens.dart';
import '../state/auth_controller.dart';
import '../widgets/auth_layout.dart';
import '../widgets/controls.dart';
import '../widgets/window_frame.dart';

/// 解锁（login.html 的 unlock 状态）：已登录会话只需输入密码。
class UnlockPage extends ConsumerStatefulWidget {
  const UnlockPage({super.key});

  @override
  ConsumerState<UnlockPage> createState() => _UnlockPageState();
}

class _UnlockPageState extends ConsumerState<UnlockPage> {
  final TextEditingController _password = TextEditingController();

  @override
  void dispose() {
    _password.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    final auth = ref.read(authControllerProvider);
    if (auth.busy || _password.text.isEmpty) {
      return;
    }
    await ref.read(authControllerProvider.notifier).unlock(_password.text);
    if (!mounted) {
      return;
    }
    if (ref.read(authControllerProvider).stage != AuthStage.unlock) {
      _password.clear();
    }
  }

  Future<void> _openReset() async {
    // 等待配置加载完成：provider 可能尚未被任何人监听，直接读 .value 会是 null，
    // 导致第一次点击无反应。
    final config = await ref.read(appConfigProvider.future);
    if (!mounted) {
      return;
    }
    await ref.read(externalLinkOpenerProvider)(
      webRoute(config.serverUrl, '/reset'),
    );
  }

  @override
  Widget build(BuildContext context) {
    final auth = ref.watch(authControllerProvider);
    final tokens = DesktopTokens.of(context);
    final account = auth.account;
    final email = account?.email ?? '';
    final initial = email.isEmpty ? '?' : email.characters.first.toUpperCase();
    final error = auth.busy ? null : auth.error;
    return AppWindowScaffold(
      width: DesktopMetrics.authWindowWidth,
      height: DesktopMetrics.authWindowHeight,
      child: AuthLayout(
        trust: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(LucideIcons.info, size: 14, color: tokens.ink500),
            const SizedBox(width: 6),
            const Text('解锁后备份会自动继续'),
          ],
        ),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Center(
              child: Container(
                width: DesktopMetrics.avatarSizeLarge,
                height: DesktopMetrics.avatarSizeLarge,
                decoration: BoxDecoration(
                  color: tokens.brand100,
                  shape: BoxShape.circle,
                ),
                alignment: Alignment.center,
                child: Text(
                  initial,
                  style: TextStyle(
                    color: tokens.brand700,
                    fontSize: DesktopMetrics.avatarSizeLarge * 0.42,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
            ),
            const AuthTitle('欢迎回来'),
            AuthSubtitle(email),
            AuthForm(
              children: [
                AuthField(
                  label: '密码',
                  error: error,
                  child: AppTextField(
                    key: const Key('unlock-password'),
                    controller: _password,
                    obscure: true,
                    enabled: !auth.busy,
                    error: error != null,
                    autofocus: true,
                    autofillHints: const <String>[AutofillHints.password],
                    textInputAction: TextInputAction.done,
                    onChanged: (_) =>
                        ref.read(authControllerProvider.notifier).clearError(),
                    onSubmitted: (_) => _submit(),
                  ),
                ),
                TokensButton(
                  key: const Key('unlock-submit'),
                  label: auth.busy ? '解锁中…' : '解锁',
                  icon: LucideIcons.lock,
                  variant: TokensButtonVariant.primary,
                  large: true,
                  block: true,
                  onPressed: auth.busy ? null : _submit,
                ),
                AuthLinks(
                  children: [
                    AuthLink(
                      key: const Key('switch-account'),
                      label: '切换账号',
                      onPressed: auth.busy
                          ? null
                          : () => ref
                                .read(authControllerProvider.notifier)
                                .switchAccount(),
                    ),
                    AuthLink(
                      key: const Key('unlock-forgot'),
                      label: '忘记密码',
                      external: true,
                      onPressed: _openReset,
                    ),
                  ],
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}
