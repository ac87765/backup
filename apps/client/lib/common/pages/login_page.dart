import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/platform/external_links.dart';
import '../../core/providers/app_providers.dart';
import '../state/auth_controller.dart';
import '../widgets/auth_layout.dart';
import '../widgets/controls.dart';

/// 登录（login.html 的 login / working / error 状态）。
class LoginPage extends ConsumerStatefulWidget {
  const LoginPage({super.key});

  @override
  ConsumerState<LoginPage> createState() => _LoginPageState();
}

class _LoginPageState extends ConsumerState<LoginPage> {
  final TextEditingController _email = TextEditingController();
  final TextEditingController _password = TextEditingController();

  @override
  void dispose() {
    _email.dispose();
    _password.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    final auth = ref.read(authControllerProvider);
    if (auth.busy) {
      return;
    }
    final email = _email.text.trim();
    final password = _password.text;
    if (email.isEmpty || password.isEmpty) {
      return;
    }
    await ref.read(authControllerProvider.notifier).login(email, password);
    if (!mounted) {
      return;
    }
    if (ref.read(authControllerProvider).stage != AuthStage.login) {
      _password.clear();
    }
  }

  Future<void> _openWeb(String path) async {
    // 等待配置加载完成：provider 可能尚未被任何人监听，直接读 .value 会是 null，
    // 导致第一次点击无反应。
    final config = await ref.read(appConfigProvider.future);
    if (!mounted) {
      return;
    }
    await ref.read(externalLinkOpenerProvider)(
      webRoute(config.serverUrl, path),
    );
  }

  @override
  Widget build(BuildContext context) {
    final auth = ref.watch(authControllerProvider);
    final busy = auth.busy;
    final error = busy ? null : auth.error;
    return AuthWindow(
      child: AuthPane(
        replayKey: busy
            ? 'working'
            : error == null
            ? 'login'
            : 'error',
        footer: const AuthTrust(text: '登录需要 1–2 秒：密码只在本机用来派生密钥'),
        children: [
          const AuthTitle('登录', key: Key('login-title')),
          const SizedBox(height: 6),
          const AuthSubtitle('使用网页版注册的账号'),
          AuthForm(
            children: [
              AuthField(
                label: '邮箱',
                child: AppTextField(
                  key: const Key('login-email'),
                  controller: _email,
                  readOnly: busy,
                  autofillHints: const <String>[AutofillHints.email],
                  textInputAction: TextInputAction.next,
                  onChanged: (_) =>
                      ref.read(authControllerProvider.notifier).clearError(),
                ),
              ),
              AuthField(
                label: '密码',
                error: error,
                child: ShakeOnTrigger(
                  trigger: error != null,
                  child: AppTextField(
                    key: const Key('login-password'),
                    controller: _password,
                    obscure: true,
                    enabled: !busy,
                    error: error != null,
                    autofillHints: const <String>[AutofillHints.password],
                    textInputAction: TextInputAction.done,
                    onChanged: (_) =>
                        ref.read(authControllerProvider.notifier).clearError(),
                    onSubmitted: (_) => _submit(),
                  ),
                ),
              ),
              TokensButton(
                key: const Key('login-submit'),
                label: busy ? '正在验证…' : '登录',
                variant: TokensButtonVariant.primary,
                large: true,
                block: true,
                busy: busy,
                progress: busy,
                onPressed: busy ? null : _submit,
              ),
              AuthLinks(
                children: [
                  AuthLink(
                    key: const Key('login-register'),
                    label: '注册账号',
                    external: true,
                    onPressed: () => _openWeb('/register'),
                  ),
                  AuthLink(
                    key: const Key('login-forgot'),
                    label: '忘记密码',
                    external: true,
                    onPressed: () => _openWeb('/reset'),
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
