import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/theme/design_tokens.dart';
import '../../desktop/widgets/update_banner.dart';

/// 最近：桥接 API 没有跨文件夹“最近文件”接口，这里如实显示占位说明。
///
/// 不为此在 client-core 里新造服务端不存在的接口（任务 5.2）。
class RecentPage extends StatelessWidget {
  const RecentPage({super.key});

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
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
              '最近',
              style: TextStyle(
                color: tokens.ink900,
                fontSize: tokens.textXl,
                fontWeight: FontWeight.w600,
              ),
            ),
          ),
        ),
        const UpdateBanner(),
        Expanded(
          child: Center(
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 440),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Container(
                    width: 56,
                    height: 56,
                    decoration: BoxDecoration(
                      color: tokens.canvas,
                      shape: BoxShape.circle,
                    ),
                    child: Icon(
                      LucideIcons.clock,
                      size: 26,
                      color: tokens.ink500,
                    ),
                  ),
                  const SizedBox(height: 16),
                  Text(
                    '“最近”暂不可用',
                    style: TextStyle(
                      color: tokens.ink900,
                      fontSize: tokens.textLg,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                  const SizedBox(height: 6),
                  Text(
                    '当前桥接接口只支持按文件夹浏览，没有跨文件夹的“最近文件”能力。'
                    '你可以在“全部文件”中按修改时间排序查看。',
                    textAlign: TextAlign.center,
                    style: TextStyle(
                      color: tokens.ink500,
                      fontSize: tokens.textSm,
                      height: 1.5,
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ],
    );
  }
}
