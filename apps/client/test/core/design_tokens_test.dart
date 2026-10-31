import 'dart:io';

import 'package:encrypted_backup_client/core/theme/design_tokens.dart';
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';

/// 读取 design/desktop/tokens.css，校验 Flutter 令牌没有漂移。
void main() {
  group('DesktopTokens 与 tokens.css 一致', () {
    late Map<String, String> css;

    setUpAll(() {
      final file = _tokensFile();
      css = _parseVariables(file.readAsStringSync());
    });

    test('亮色颜色逐项一致', () {
      const tokens = DesktopTokens.light;
      final colors = <String, Color>{
        'brand-50': tokens.brand50,
        'brand-100': tokens.brand100,
        'brand-600': tokens.brand600,
        'brand-700': tokens.brand700,
        'ink-900': tokens.ink900,
        'ink-600': tokens.ink600,
        'ink-500': tokens.ink500,
        'ink-400': tokens.ink400,
        'line': tokens.line,
        'line-strong': tokens.lineStrong,
        'canvas': tokens.canvas,
        'canvas-strong': tokens.canvasStrong,
        'surface': tokens.surface,
        'danger-50': tokens.danger50,
        'danger-600': tokens.danger600,
        'danger-700': tokens.danger700,
        'success-50': tokens.success50,
        'success-600': tokens.success600,
        'success-700': tokens.success700,
        'warning-50': tokens.warning50,
        'warning-600': tokens.warning600,
        'file-folder': tokens.fileFolder,
        'file-image': tokens.fileImage,
        'file-video': tokens.fileVideo,
        'file-pdf': tokens.filePdf,
        'file-doc': tokens.fileDoc,
        'file-sheet': tokens.fileSheet,
        'file-slides': tokens.fileSlides,
        'file-archive': tokens.fileArchive,
        'file-other': tokens.fileOther,
      };
      for (final entry in colors.entries) {
        final raw = css[entry.key];
        expect(raw, isNotNull, reason: 'tokens.css 缺少 --${entry.key}');
        expect(
          entry.value.toARGB32(),
          _parseColor(raw!).toARGB32(),
          reason: '--${entry.key} 与 DesktopTokens 不一致',
        );
      }
    });

    test('圆角、间距、字号一致', () {
      const tokens = DesktopTokens.light;
      expect(tokens.radiusCard, _parsePx(css['radius-card']!));
      expect(tokens.radiusControl, _parsePx(css['radius-control']!));
      final spacings = <String, double>{
        'space-1': tokens.space1,
        'space-2': tokens.space2,
        'space-3': tokens.space3,
        'space-4': tokens.space4,
        'space-5': tokens.space5,
        'space-6': tokens.space6,
        'space-8': tokens.space8,
      };
      for (final entry in spacings.entries) {
        expect(tokens.space1, 4, reason: '间距基数为 4px');
        expect(entry.value, _parsePx(css[entry.key]!), reason: entry.key);
      }
      final fontSizes = <String, double>{
        'text-xs': tokens.textXs,
        'text-sm': tokens.textSm,
        'text-md': tokens.textMd,
        'text-lg': tokens.textLg,
        'text-xl': tokens.textXl,
        'text-2xl': tokens.text2xl,
      };
      for (final entry in fontSizes.entries) {
        expect(entry.value, _parsePx(css[entry.key]!), reason: entry.key);
      }
    });

    test('阴影与动效令牌存在', () {
      const tokens = DesktopTokens.light;
      expect(tokens.shadowPop, isNotEmpty);
      expect(tokens.shadowWindow, hasLength(2));
      expect(tokens.durationFast, const Duration(milliseconds: 120));
      expect(tokens.durationBase, const Duration(milliseconds: 180));
      expect(tokens.durationSlow, const Duration(milliseconds: 260));
      expect(
        tokens.easeOut.transform(0.5),
        const Cubic(0.2, 0, 0, 1).transform(0.5),
      );
    });

    test('暗色对照色板保持相同的尺寸令牌且颜色不同', () {
      const light = DesktopTokens.light;
      const dark = DesktopTokens.dark;
      expect(dark.radiusCard, light.radiusCard);
      expect(dark.radiusControl, light.radiusControl);
      expect(dark.space4, light.space4);
      expect(dark.textSm, light.textSm);
      expect(dark.canvas, isNot(light.canvas));
      expect(dark.surface, isNot(light.surface));
      expect(dark.ink900, isNot(light.ink900));
      expect(dark.brand600, isNot(light.brand600));
    });
  });
}

File _tokensFile() {
  for (final path in const [
    '../../design/desktop/tokens.css',
    'design/desktop/tokens.css',
  ]) {
    final file = File(path);
    if (file.existsSync()) {
      return file;
    }
  }
  throw StateError(
    '找不到 design/desktop/tokens.css（请在 apps/client 下运行 flutter test）',
  );
}

Map<String, String> _parseVariables(String css) {
  final variables = <String, String>{};
  final pattern = RegExp(r'--([a-zA-Z0-9-]+)\s*:\s*([^;]+);');
  for (final match in pattern.allMatches(css)) {
    final name = match.group(1);
    final value = match.group(2);
    if (name != null && value != null) {
      variables[name] = value.trim();
    }
  }
  return variables;
}

Color _parseColor(String value) {
  final hex = value.trim();
  if (!RegExp(r'^#[0-9a-fA-F]{6}$').hasMatch(hex)) {
    throw FormatException('不支持的颜色格式：$value');
  }
  return Color(0xFF000000 | int.parse(hex.substring(1), radix: 16));
}

double _parsePx(String value) {
  return double.parse(value.trim().replaceAll('px', ''));
}
