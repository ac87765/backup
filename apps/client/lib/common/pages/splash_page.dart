import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../state/auth_controller.dart';
import '../widgets/window_frame.dart';
import '../../core/theme/design_tokens.dart';

/// 启动页：初始化 Rust 核心并恢复会话。
class SplashPage extends ConsumerWidget {
  const SplashPage({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = DesktopTokens.of(context);
    final auth = ref.watch(authControllerProvider);
    return ColoredBox(
      color: tokens.designCanvas,
      child: Center(
        child: SizedBox(
          width: DesktopMetrics.authWindowWidth,
          height: DesktopMetrics.authWindowHeight,
          child: WindowFrame(
            child: Column(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                BrandLogo(size: DesktopMetrics.logoSizeLarge),
                SizedBox(height: tokens.space4),
                Text('加密备份', style: Theme.of(context).textTheme.titleLarge),
                SizedBox(height: tokens.space2),
                Text(
                  '文件在这台电脑上加密后才上传',
                  style: Theme.of(context).textTheme.bodySmall,
                ),
                SizedBox(height: tokens.space6),
                if (auth.stage == AuthStage.initializing)
                  SizedBox(
                    width: 20,
                    height: 20,
                    child: CircularProgressIndicator(
                      strokeWidth: 2,
                      color: tokens.brand600,
                    ),
                  )
                else if (auth.stage == AuthStage.failure)
                  _Failure(error: auth.error)
                else
                  const SizedBox.shrink(),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _Failure extends ConsumerWidget {
  const _Failure({this.error});

  final String? error;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = DesktopTokens.of(context);
    return Padding(
      padding: EdgeInsets.symmetric(horizontal: tokens.space6),
      child: Column(
        children: [
          const Icon(LucideIcons.alertTriangle, size: 20),
          SizedBox(height: tokens.space2),
          Text(
            error ?? '启动失败，请重试',
            textAlign: TextAlign.center,
            style: TextStyle(color: tokens.danger700, fontSize: tokens.textXs),
          ),
        ],
      ),
    );
  }
}
