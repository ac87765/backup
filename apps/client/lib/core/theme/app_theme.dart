import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';

import 'design_tokens.dart';

/// 应用主题：所有颜色/圆角/字号都来自 [DesktopTokens]。
abstract final class AppTheme {
  /// 亮色主题。
  static ThemeData get light => _build(DesktopTokens.light);

  /// 暗色主题（tokens.css 尚未定义暗色，见 [DesktopTokens.dark]）。
  static ThemeData get dark => _build(DesktopTokens.dark);

  static ThemeData _build(DesktopTokens tokens) {
    final base = ThemeData(
      useMaterial3: true,
      brightness: tokens.brightness,
      scaffoldBackgroundColor: tokens.canvas,
      fontFamily: _fontFamily(),
      fontFamilyFallback: _fontFamilyFallback(),
      colorScheme: ColorScheme(
        brightness: tokens.brightness,
        primary: tokens.brand600,
        onPrimary: tokens.onBrand,
        primaryContainer: tokens.brand100,
        onPrimaryContainer: tokens.brand700,
        secondary: tokens.brand700,
        onSecondary: tokens.onBrand,
        error: tokens.danger600,
        onError: tokens.onBrand,
        surface: tokens.surface,
        onSurface: tokens.ink900,
        outline: tokens.lineStrong,
      ),
    );

    final text = base.textTheme;
    TextStyle? style(
      TextStyle? source,
      double size,
      Color color, {
      FontWeight? weight,
    }) {
      return source?.copyWith(
        fontSize: size,
        color: color,
        fontWeight: weight,
        height: 1.45,
      );
    }

    return base.copyWith(
      textTheme: text.copyWith(
        displayLarge: style(
          text.displayLarge,
          tokens.text2xl,
          tokens.ink900,
          weight: FontWeight.w600,
        ),
        headlineSmall: style(
          text.headlineSmall,
          tokens.textXl,
          tokens.ink900,
          weight: FontWeight.w600,
        ),
        titleLarge: style(
          text.titleLarge,
          tokens.textLg,
          tokens.ink900,
          weight: FontWeight.w600,
        ),
        titleMedium: style(
          text.titleMedium,
          tokens.textMd,
          tokens.ink900,
          weight: FontWeight.w600,
        ),
        bodyLarge: style(text.bodyLarge, tokens.textMd, tokens.ink900),
        bodyMedium: style(text.bodyMedium, tokens.textSm, tokens.ink900),
        bodySmall: style(text.bodySmall, tokens.textXs, tokens.ink500),
        labelLarge: style(
          text.labelLarge,
          tokens.textSm,
          tokens.ink900,
          weight: FontWeight.w500,
        ),
        labelSmall: style(text.labelSmall, tokens.textXs, tokens.ink500),
      ),
      dividerTheme: DividerThemeData(
        color: tokens.line,
        thickness: 1,
        space: 1,
      ),
      iconTheme: IconThemeData(color: tokens.ink600, size: 18),
    );
  }

  /// Windows：Segoe UI Variable + 微软雅黑回落；macOS：系统字体（SF Pro +
  /// 苹方由系统负责），因此返回 null 让 Flutter 使用平台默认字体。
  static String? _fontFamily() {
    if (defaultTargetPlatform == TargetPlatform.windows) {
      return 'Segoe UI Variable Text';
    }
    return null;
  }

  /// Windows：Segoe UI Variable 没有中文字形，回落到微软雅黑（design
  /// README 的字体约定）。macOS 使用系统字体，由平台负责苹方回落。
  static List<String>? _fontFamilyFallback() {
    if (defaultTargetPlatform == TargetPlatform.windows) {
      return const <String>['Microsoft YaHei UI', 'Microsoft YaHei'];
    }
    return null;
  }
}
