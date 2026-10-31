import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/platform/external_links.dart';
import '../../core/providers/app_providers.dart';
import '../state/auth_controller.dart';
import '../widgets/auth_layout.dart';
import '../widgets/controls.dart';

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
    final account = auth.account;
    final email = account?.email ?? '';
    final initial = email.isEmpty ? '?' : email.characters.first.toUpperCase();
    final error = auth.busy ? null : auth.error;
    return AuthWindow(
      child: AuthPane(
        replayKey: auth.busy
            ? 'unlocking'
            : error == null
            ? 'unlock'
            : 'unlock-error',
        footer: const AuthTrust(text: '可在设置中开启“在这台电脑上保持解锁”'),
        children: [
          const AuthTitle('欢迎回来', key: Key('unlock-title')),
          const SizedBox(height: 6),
          const AuthSubtitle('输入密码解锁，备份会自动继续'),
          AuthAccountCard(email: email, initial: initial),
          AuthForm(
            top: 20,
            children: [
              AuthField(
                label: '密码',
                error: error,
                child: ShakeOnTrigger(
                  trigger: error != null,
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
              ),
              TokensButton(
                key: const Key('unlock-submit'),
                label: auth.busy ? '解锁中…' : '解锁',
                icon: LucideIcons.lock,
                variant: TokensButtonVariant.primary,
                large: true,
                block: true,
                busy: auth.busy,
                progress: auth.busy,
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
    );
  }
}
