import 'package:flutter/material.dart';

import '../../core/theme/design_tokens.dart';

/// 占位页面：第 4 组只搭工程骨架，真实界面在第 5、6 组实现。
class PlaceholderPage extends StatelessWidget {
  const PlaceholderPage({
    required this.title,
    required this.description,
    required this.icon,
    super.key,
  });

  final String title;
  final String description;
  final IconData icon;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Center(
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 460),
        child: Padding(
          padding: EdgeInsets.all(tokens.space6),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Container(
                width: 44,
                height: 44,
                decoration: BoxDecoration(
                  color: tokens.brand50,
                  borderRadius: BorderRadius.circular(tokens.radiusControl),
                ),
                child: Icon(icon, color: tokens.brand700, size: 22),
              ),
              SizedBox(height: tokens.space4),
              Text(title, style: Theme.of(context).textTheme.headlineSmall),
              SizedBox(height: tokens.space2),
              Text(
                description,
                style: Theme.of(context).textTheme.bodyMedium
                    ?.copyWith(color: tokens.ink600),
              ),
              SizedBox(height: tokens.space5),
              Container(
                padding: EdgeInsets.symmetric(
                  horizontal: tokens.space3,
                  vertical: tokens.space2,
                ),
                decoration: BoxDecoration(
                  color: tokens.canvas,
                  borderRadius: BorderRadius.circular(tokens.radiusControl),
                  border: Border.all(color: tokens.line),
                ),
                child: Text(
                  '本页为工程占位，第 5、6 组实现真实界面。',
                  style: Theme.of(context).textTheme.bodySmall,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
