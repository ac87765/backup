import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/theme/design_tokens.dart';
import '../state/auth_controller.dart';
import '../widgets/controls.dart';

/// 侧栏“安全与隐私”：端到端加密说明、保持解锁风险、本机数据与恢复密钥。
class SecurityPage extends ConsumerWidget {
  const SecurityPage({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final tokens = DesktopTokens.of(context);
    final auth = ref.watch(authControllerProvider);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Container(
          height: DesktopMetrics.toolbarHeight,
          padding: const EdgeInsets.symmetric(horizontal: 20),
          decoration: BoxDecoration(
            border: Border(bottom: BorderSide(color: tokens.line)),
          ),
          child: Align(
            alignment: Alignment.centerLeft,
            child: Text(
              '安全与隐私',
              style: TextStyle(
                color: tokens.ink900,
                fontSize: tokens.textXl,
                fontWeight: FontWeight.w600,
              ),
            ),
          ),
        ),
        Expanded(
          child: SingleChildScrollView(
            padding: const EdgeInsets.symmetric(horizontal: 32, vertical: 24),
            child: Center(
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 820),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Text(
                      '你的文件，只有你能打开',
                      style: TextStyle(
                        color: tokens.ink900,
                        fontSize: tokens.textXl,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                    const SizedBox(height: 8),
                    Text(
                      '文件在这台电脑上加密后才上传；密码与主密钥不会离开本机，'
                      '服务器和云厂商都看不到明文内容与文件名。',
                      style: TextStyle(
                        color: tokens.ink600,
                        fontSize: tokens.textSm,
                        height: 1.55,
                      ),
                    ),
                    const SizedBox(height: 20),
                    _VisibilityCard(tokens: tokens),
                    const SizedBox(height: 12),
                    _SectionCard(
                      tokens: tokens,
                      icon: LucideIcons.laptop,
                      title: '这台电脑上保存了什么',
                      body:
                          '仅保存文件路径、备份记录与传输续传元数据（明文，与系统文件索引同级），'
                          '不保存文件内容或任何密钥。云端看不到这些本地路径。',
                    ),
                    const SizedBox(height: 12),
                    _SectionCard(
                      tokens: tokens,
                      icon: LucideIcons.lock,
                      title: '在这台电脑上保持解锁',
                      trailing: TokensPill(
                        label: auth.keepUnlocked ? '已开启' : '默认关闭',
                        tone: auth.keepUnlocked
                            ? PillTone.warn
                            : PillTone.paused,
                      ),
                      body: auth.keepUnlocked
                          ? '已开启：主密钥由系统钥匙串保管，任何能登录这台电脑系统账号的人'
                                '都可以打开加密备份。退出登录或修改密码时自动关闭。'
                          : '默认关闭：每次启动应用都需要输入密码解锁。开启后主密钥由系统'
                                '钥匙串保管，重启电脑后无需输入密码即可继续备份。',
                    ),
                    const SizedBox(height: 12),
                    _SectionCard(
                      tokens: tokens,
                      icon: LucideIcons.shieldCheck,
                      title: '恢复密钥',
                      body:
                          '恢复密钥与密码同等重要，只保存在你自己手中。服务端永远不会向你索要'
                          '密码或恢复密钥；忘记密码时可在网页版用恢复密钥重置。',
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ],
    );
  }
}

class _VisibilityCard extends StatelessWidget {
  const _VisibilityCard({required this.tokens});

  final DesktopTokens tokens;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(20),
      decoration: BoxDecoration(
        color: tokens.surface,
        border: Border.all(color: tokens.line),
        borderRadius: BorderRadius.circular(tokens.radiusCard),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Expanded(
            child: _list('服务器能看到', LucideIcons.info, tokens.ink500, const [
              '账号邮箱',
              '文件夹树结构（无名称）',
              '密文大小与块数',
              '创建、修改、删除时间',
            ]),
          ),
          Container(
            width: 1,
            height: 150,
            margin: const EdgeInsets.symmetric(horizontal: 24),
            color: tokens.line,
          ),
          Expanded(
            child: _list('服务器看不到', LucideIcons.lock, tokens.brand600, const [
              '文件内容',
              '文件名与类型',
              '原始修改时间',
              '密码与主密钥',
            ]),
          ),
        ],
      ),
    );
  }

  Widget _list(String title, IconData icon, Color color, List<String> items) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Icon(icon, size: 18, color: color),
            const SizedBox(width: 8),
            Text(
              title,
              style: TextStyle(
                color: tokens.ink900,
                fontSize: tokens.textSm,
                fontWeight: FontWeight.w600,
              ),
            ),
          ],
        ),
        const SizedBox(height: 12),
        for (final item in items)
          Padding(
            padding: const EdgeInsets.only(bottom: 8),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Padding(
                  padding: const EdgeInsets.only(top: 6),
                  child: Container(
                    width: 6,
                    height: 6,
                    decoration: BoxDecoration(
                      color: color,
                      shape: BoxShape.circle,
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                Expanded(
                  child: Text(
                    item,
                    style: TextStyle(
                      color: tokens.ink600,
                      fontSize: tokens.textSm,
                    ),
                  ),
                ),
              ],
            ),
          ),
      ],
    );
  }
}

class _SectionCard extends StatelessWidget {
  const _SectionCard({
    required this.tokens,
    required this.icon,
    required this.title,
    required this.body,
    this.trailing,
  });

  final DesktopTokens tokens;
  final IconData icon;
  final String title;
  final String body;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    return Container(
      padding: const EdgeInsets.all(20),
      decoration: BoxDecoration(
        color: tokens.surface,
        border: Border.all(color: tokens.line),
        borderRadius: BorderRadius.circular(tokens.radiusCard),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Container(
            width: 32,
            height: 32,
            decoration: BoxDecoration(
              color: tokens.brand50,
              borderRadius: BorderRadius.circular(6),
            ),
            child: Icon(icon, size: 18, color: tokens.brand600),
          ),
          const SizedBox(width: 14),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Expanded(
                      child: Text(
                        title,
                        style: TextStyle(
                          color: tokens.ink900,
                          fontSize: tokens.textSm,
                          fontWeight: FontWeight.w600,
                        ),
                      ),
                    ),
                    ?trailing,
                  ],
                ),
                const SizedBox(height: 4),
                Text(
                  body,
                  style: TextStyle(
                    color: tokens.ink600,
                    fontSize: tokens.textSm,
                    height: 1.55,
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
