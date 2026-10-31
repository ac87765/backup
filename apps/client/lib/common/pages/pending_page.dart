import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/theme/design_tokens.dart';
import '../state/auth_controller.dart';
import '../widgets/auth_layout.dart';
import '../widgets/controls.dart';
import '../widgets/window_frame.dart';

/// 待审核账号（login.html 的 pending 状态）：没有上传/新建入口。
class PendingPage extends ConsumerWidget {
  const PendingPage({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final auth = ref.watch(authControllerProvider);
    final tokens = DesktopTokens.of(context);
    final email = auth.account?.email ?? '';
    return AppWindowScaffold(
      width: DesktopMetrics.authWindowWidth,
      height: DesktopMetrics.authWindowHeight,
      child: AuthLayout(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Center(
              child: Container(
                width: DesktopMetrics.avatarSizeLarge,
                height: DesktopMetrics.avatarSizeLarge,
                decoration: BoxDecoration(
                  color: tokens.warning50,
                  borderRadius: BorderRadius.circular(
                    DesktopMetrics.avatarSizeLarge * 0.28,
                  ),
                ),
                child: Icon(
                  LucideIcons.hourglass,
                  size: 28,
                  color: tokens.warning600,
                ),
              ),
            ),
            const AuthTitle('正在等待管理员审核'),
            AuthSubtitle(
              email.isEmpty
                  ? '账号通过审核后即可上传和备份文件。'
                  : '账号通过审核后即可上传和备份文件。审核结果会发送到 $email。',
            ),
            const SizedBox(height: 24),
            AuthForm(
              children: [
                TokensButton(
                  key: const Key('pending-refresh'),
                  label: '重新检查',
                  icon: LucideIcons.refreshCw,
                  large: true,
                  block: true,
                  onPressed: auth.busy
                      ? null
                      : () => ref
                            .read(authControllerProvider.notifier)
                            .refreshAccount(),
                ),
                TokensButton(
                  key: const Key('pending-logout'),
                  label: '退出登录',
                  variant: TokensButtonVariant.ghost,
                  large: true,
                  block: true,
                  onPressed: auth.busy
                      ? null
                      : () =>
                            ref.read(authControllerProvider.notifier).logout(),
                ),
              ],
            ),
            if (auth.error != null) ...[
              const SizedBox(height: 12),
              Text(
                auth.error!,
                textAlign: TextAlign.center,
                style: TextStyle(
                  color: tokens.danger700,
                  fontSize: tokens.textXs,
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }
}
