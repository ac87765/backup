import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/theme/design_tokens.dart';

/// 登录窗口左侧 400 宽品牌面板（login.html `.auth-brand`）。
///
/// 做成独立组件：桌面认证页使用，将来 iOS 改用不同布局时可以不引用。
/// 渐变、网格、插画全部用 Flutter 自绘（CustomPainter / Container 组合），
/// 不引入图片资源。
///
/// 动效参数来自 login.html 顶部注释：
/// - 三张文件卡片与云朵：6s 上下浮动 6px，错开 0.4s（文件 0 / .4 / .8s，
///   云朵 1.2s）；
/// - 锁徽章：3s 呼吸光晕；
/// - 白色小方块沿两段虚线流动：2.4s 循环，a 0s / b 1.2s / c .6s / d 1.8s。
/// 系统开启“减少动态效果”时停止控制器并显示静止画面，小方块隐藏。
class AuthBrandPanel extends StatefulWidget {
  const AuthBrandPanel({super.key});

  @override
  State<AuthBrandPanel> createState() => _AuthBrandPanelState();
}

class _AuthBrandPanelState extends State<AuthBrandPanel>
    with TickerProviderStateMixin {
  static const Duration _floatDuration = Duration(seconds: 6);
  static const Duration _glowDuration = Duration(seconds: 3);
  static const Duration _flowDuration = Duration(milliseconds: 2400);

  late final AnimationController _float = AnimationController(
    vsync: this,
    duration: _floatDuration,
  );
  late final AnimationController _glow = AnimationController(
    vsync: this,
    duration: _glowDuration,
  );
  late final AnimationController _flow = AnimationController(
    vsync: this,
    duration: _flowDuration,
  );
  bool? _reduceMotion;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final reduce = MediaQuery.disableAnimationsOf(context);
    if (_reduceMotion == reduce) {
      return;
    }
    _reduceMotion = reduce;
    if (reduce) {
      _float.stop();
      _glow.stop();
      _flow.stop();
      // 静止画面：浮动回到 0、光晕回到最弱的一帧；小方块在 build 里隐藏。
      _float.value = 0;
      _glow.value = 0;
      _flow.value = 0;
    } else {
      _float.repeat();
      _glow.repeat();
      _flow.repeat();
    }
  }

  @override
  void dispose() {
    _float.dispose();
    _glow.dispose();
    _flow.dispose();
    super.dispose();
  }

  bool get _reduce => _reduceMotion ?? false;

  double _floatOffset(double delaySeconds) {
    if (_reduce) {
      return 0;
    }
    final t = (_float.value + delaySeconds / _floatDuration.inSeconds) % 1.0;
    // CSS @keyframes float：0%/100% 在 0，50% 在 -6px，逐段 ease-in-out。
    final phase = t < 0.5 ? t * 2 : (1 - t) * 2;
    return -6 * Curves.easeInOut.transform(phase);
  }

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Container(
      width: DesktopMetrics.authBrandWidth,
      color: tokens.brand600,
      child: Stack(
        fit: StackFit.expand,
        children: [
          // linear-gradient(160deg, brand-600, brand-700)
          DecoratedBox(
            decoration: BoxDecoration(
              gradient: LinearGradient(
                begin: const Alignment(-0.34, -0.94),
                end: const Alignment(0.34, 0.94),
                colors: <Color>[tokens.brand600, tokens.brand700],
              ),
            ),
          ),
          // 两层径向高光：左上白光、右下暗角。
          DecoratedBox(
            decoration: BoxDecoration(
              gradient: RadialGradient(
                center: Alignment.topLeft,
                radius: 1.35,
                colors: <Color>[
                  Colors.white.withValues(alpha: 0.16),
                  Colors.transparent,
                ],
                stops: const <double>[0, 0.6],
              ),
            ),
          ),
          DecoratedBox(
            decoration: BoxDecoration(
              gradient: RadialGradient(
                center: Alignment.bottomRight,
                radius: 1.2,
                colors: <Color>[
                  Colors.black.withValues(alpha: 0.18),
                  Colors.transparent,
                ],
                stops: const <double>[0, 0.6],
              ),
            ),
          ),
          // 28px 细网格，顶部到 85% 渐隐。
          ShaderMask(
            blendMode: BlendMode.dstIn,
            shaderCallback: (rect) {
              return const LinearGradient(
                begin: Alignment.topCenter,
                end: Alignment.bottomCenter,
                colors: <Color>[Colors.white, Colors.transparent],
                stops: <double>[0, 0.85],
              ).createShader(rect);
            },
            child: const CustomPaint(painter: _GridPainter()),
          ),
          Padding(
            padding: const EdgeInsets.fromLTRB(
              DesktopMetrics.authBrandPadding,
              DesktopMetrics.authBrandPadding,
              DesktopMetrics.authBrandPadding,
              DesktopMetrics.authBrandPaddingBottom,
            ),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const _BrandWordmark(),
                const SizedBox(height: 28),
                const Text(
                  '你的文件，\n只有你能打开',
                  style: TextStyle(
                    color: Colors.white,
                    fontSize: 24,
                    fontWeight: FontWeight.w600,
                    height: 1.3,
                  ),
                ),
                const SizedBox(height: 8),
                Text(
                  '文件在本机加密后才上传，云端只保存密文。',
                  style: TextStyle(
                    color: Colors.white.withValues(alpha: 0.82),
                    fontSize: 13,
                    height: 1.6,
                  ),
                ),
                const SizedBox(height: 22),
                _AuthScene(panel: this, tokens: tokens),
                const SizedBox(height: 18),
                const Spacer(),
                const _BulletList(),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _BrandWordmark extends StatelessWidget {
  const _BrandWordmark();

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        Container(
          width: 32,
          height: 32,
          decoration: BoxDecoration(
            color: Colors.white,
            borderRadius: BorderRadius.circular(32 * 0.28),
          ),
          child: Icon(LucideIcons.lock, size: 18, color: tokens.brand600),
        ),
        const SizedBox(width: 10),
        const Text(
          '加密备份',
          style: TextStyle(
            color: Colors.white,
            fontSize: 16,
            fontWeight: FontWeight.w600,
          ),
        ),
      ],
    );
  }
}

class _BulletList extends StatelessWidget {
  const _BulletList();

  static const List<(IconData, String)> _items = <(IconData, String)>[
    (LucideIcons.laptop, '在本机加密，密钥不离开这台电脑'),
    (LucideIcons.refreshCw, '文件夹自动增量备份，保留旧版本'),
    (LucideIcons.shieldCheck, '服务器无法看到你的密码和文件内容'),
  ];

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        for (var index = 0; index < _items.length; index += 1) ...[
          if (index > 0) const SizedBox(height: 12),
          Row(
            children: [
              Container(
                width: 28,
                height: 28,
                decoration: BoxDecoration(
                  color: Colors.white.withValues(alpha: 0.14),
                  borderRadius: BorderRadius.circular(8),
                ),
                child: Icon(_items[index].$1, size: 15, color: Colors.white),
              ),
              const SizedBox(width: 10),
              Expanded(
                child: Text(
                  _items[index].$2,
                  style: TextStyle(
                    color: Colors.white.withValues(alpha: 0.92),
                    fontSize: 13,
                  ),
                ),
              ),
            ],
          ),
        ],
      ],
    );
  }
}

/// 文件 → 本机加密（锁）→ 云端的插画。
class _AuthScene extends StatelessWidget {
  const _AuthScene({required this.panel, required this.tokens});

  final _AuthBrandPanelState panel;
  final DesktopTokens tokens;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      height: 150,
      child: Stack(
        clipBehavior: Clip.none,
        children: [
          Positioned.fill(
            child: CustomPaint(
              painter: _SceneLinesPainter(
                color: Colors.white.withValues(alpha: 0.45),
              ),
            ),
          ),
          Positioned(
            left: 4,
            top: 30,
            width: 92,
            height: 96,
            child: Stack(
              clipBehavior: Clip.none,
              children: [
                _fileCard(
                  panel,
                  icon: LucideIcons.image,
                  left: 0,
                  top: 14,
                  angle: -8,
                  opacity: 0.78,
                  delaySeconds: 0,
                ),
                _fileCard(
                  panel,
                  icon: LucideIcons.sheet,
                  left: 14,
                  top: 6,
                  angle: -2,
                  opacity: 0.9,
                  delaySeconds: 0.4,
                ),
                _fileCard(
                  panel,
                  icon: LucideIcons.fileText,
                  left: 28,
                  top: 0,
                  angle: 5,
                  opacity: 1,
                  delaySeconds: 0.8,
                ),
              ],
            ),
          ),
          Positioned(
            left: 148,
            top: 44,
            child: AnimatedBuilder(
              animation: panel._glow,
              builder: (context, child) {
                final t = panel._reduce ? 0.0 : panel._glow.value;
                final breathe = (1 - math.cos(2 * math.pi * t)) / 2;
                final ring = 8 + 6 * breathe;
                final alpha = 0.12 - 0.06 * breathe;
                return Container(
                  width: 64,
                  height: 64,
                  decoration: BoxDecoration(
                    color: Colors.white,
                    borderRadius: BorderRadius.circular(18),
                    boxShadow: <BoxShadow>[
                      BoxShadow(
                        color: Colors.white.withValues(alpha: alpha),
                        spreadRadius: ring,
                      ),
                      BoxShadow(
                        color: Colors.black.withValues(alpha: 0.2),
                        blurRadius: 26,
                        offset: const Offset(0, 10),
                      ),
                    ],
                  ),
                  child: Icon(
                    LucideIcons.lock,
                    size: 28,
                    color: tokens.brand600,
                  ),
                );
              },
            ),
          ),
          Positioned(
            right: 0,
            top: 36,
            child: AnimatedBuilder(
              animation: panel._float,
              builder: (context, child) {
                return Transform.translate(
                  offset: Offset(0, panel._floatOffset(1.2)),
                  child: child,
                );
              },
              child: Container(
                width: 84,
                height: 84,
                decoration: BoxDecoration(
                  color: Colors.white.withValues(alpha: 0.14),
                  shape: BoxShape.circle,
                ),
                child: const Icon(
                  LucideIcons.cloud,
                  size: 34,
                  color: Colors.white,
                ),
              ),
            ),
          ),
          if (!panel._reduce) ...[
            _packet(
              panel,
              key: const ValueKey<String>('auth-packet-a'),
              startX: 100,
              travel: 44,
              delaySeconds: 0,
            ),
            _packet(
              panel,
              key: const ValueKey<String>('auth-packet-b'),
              startX: 100,
              travel: 44,
              delaySeconds: 1.2,
            ),
            _packet(
              panel,
              key: const ValueKey<String>('auth-packet-c'),
              startX: 218,
              travel: 52,
              delaySeconds: 0.6,
            ),
            _packet(
              panel,
              key: const ValueKey<String>('auth-packet-d'),
              startX: 218,
              travel: 52,
              delaySeconds: 1.8,
            ),
          ],
        ],
      ),
    );
  }

  Widget _fileCard(
    _AuthBrandPanelState panel, {
    required IconData icon,
    required double left,
    required double top,
    required double angle,
    required double opacity,
    required double delaySeconds,
  }) {
    return Positioned(
      left: left,
      top: top,
      child: AnimatedBuilder(
        animation: panel._float,
        builder: (context, child) {
          return Transform.translate(
            offset: Offset(0, panel._floatOffset(delaySeconds)),
            child: Transform.rotate(angle: angle * math.pi / 180, child: child),
          );
        },
        child: Opacity(
          opacity: opacity,
          child: Container(
            width: 64,
            height: 78,
            decoration: BoxDecoration(
              color: Colors.white,
              borderRadius: BorderRadius.circular(10),
              boxShadow: <BoxShadow>[
                BoxShadow(
                  color: Colors.black.withValues(alpha: 0.18),
                  blurRadius: 20,
                  offset: const Offset(0, 8),
                ),
              ],
            ),
            child: Icon(icon, size: 22, color: tokens.brand600),
          ),
        ),
      ),
    );
  }

  Widget _packet(
    _AuthBrandPanelState panel, {
    required Key key,
    required double startX,
    required double travel,
    required double delaySeconds,
  }) {
    return Positioned(
      key: key,
      left: startX,
      top: 74,
      child: AnimatedBuilder(
        animation: panel._flow,
        builder: (context, child) {
          final t = (panel._flow.value + delaySeconds / 2.4) % 1.0;
          final opacity = t < 0.15
              ? t / 0.15 * 0.9
              : t < 0.85
              ? 0.9
              : (1 - t) / 0.15 * 0.9;
          return Opacity(
            opacity: opacity,
            child: Transform.translate(
              offset: Offset(travel * t, 0),
              child: child,
            ),
          );
        },
        child: Container(
          width: 8,
          height: 8,
          decoration: BoxDecoration(
            color: Colors.white,
            borderRadius: BorderRadius.circular(2),
          ),
        ),
      ),
    );
  }
}

class _GridPainter extends CustomPainter {
  const _GridPainter();

  @override
  void paint(Canvas canvas, Size size) {
    final paint = Paint()
      ..color = Colors.white.withValues(alpha: 0.06)
      ..strokeWidth = 1;
    const step = 28.0;
    for (var x = 0.0; x <= size.width; x += step) {
      canvas.drawLine(Offset(x, 0), Offset(x, size.height), paint);
    }
    for (var y = 0.0; y <= size.height; y += step) {
      canvas.drawLine(Offset(0, y), Offset(size.width, y), paint);
    }
  }

  @override
  bool shouldRepaint(_GridPainter oldDelegate) => false;
}

class _SceneLinesPainter extends CustomPainter {
  const _SceneLinesPainter({required this.color});

  final Color color;

  @override
  void paint(Canvas canvas, Size size) {
    final paint = Paint()
      ..color = color
      ..strokeWidth = 2
      ..strokeCap = StrokeCap.butt;
    _dash(canvas, 96, 148, paint);
    _dash(canvas, 212, 266, paint);
  }

  void _dash(Canvas canvas, double from, double to, Paint paint) {
    var x = from;
    while (x < to) {
      final end = math.min(x + 4, to);
      canvas.drawLine(Offset(x, 78), Offset(end, 78), paint);
      x += 10;
    }
  }

  @override
  bool shouldRepaint(_SceneLinesPainter oldDelegate) =>
      oldDelegate.color != color;
}
