import 'package:flutter/material.dart';

/// 桌面客户端设计令牌。
///
/// 来源：`design/desktop/tokens.css`（与 Web `web/src/index.css` 一致）。本类
/// 只做“原样搬运”：颜色、圆角、间距、字号、阴影、动效的数值必须与
/// tokens.css 一致；`test/core/design_tokens_test.dart` 会读取 CSS 校验，
/// 防止两处漂移。
///
/// 注意：tokens.css 目前只定义了亮色。暗色实例是为了满足桌面端“亮/暗都要”
/// 而补充的对照色板，圆角/间距/字号/阴影与亮色完全相同；颜色值需审查者确认
/// 后再作为设计令牌冻结（见汇报“待决定事项”）。
@immutable
class DesktopTokens {
  const DesktopTokens({
    required this.brightness,
    required this.brand50,
    required this.brand100,
    required this.brand600,
    required this.brand700,
    required this.ink900,
    required this.ink600,
    required this.ink500,
    required this.ink400,
    required this.line,
    required this.lineStrong,
    required this.canvas,
    required this.canvasStrong,
    required this.surface,
    required this.danger50,
    required this.danger600,
    required this.danger700,
    required this.success50,
    required this.success600,
    required this.success700,
    required this.warning50,
    required this.warning600,
    required this.fileFolder,
    required this.fileImage,
    required this.fileVideo,
    required this.filePdf,
    required this.fileDoc,
    required this.fileSheet,
    required this.fileSlides,
    required this.fileArchive,
    required this.fileOther,
    required this.radiusCard,
    required this.radiusControl,
    required this.space1,
    required this.space2,
    required this.space3,
    required this.space4,
    required this.space5,
    required this.space6,
    required this.space8,
    required this.textXs,
    required this.textSm,
    required this.textMd,
    required this.textLg,
    required this.textXl,
    required this.text2xl,
    required this.shadowPop,
    required this.shadowWindow,
    required this.easeOut,
    required this.durationFast,
    required this.durationBase,
    required this.durationSlow,
  });

  final Brightness brightness;

  // ---- 品牌色 ----
  final Color brand50;
  final Color brand100;
  final Color brand600;
  final Color brand700;

  // ---- 文字 ----
  final Color ink900;
  final Color ink600;
  final Color ink500;
  final Color ink400;

  // ---- 边框与背景 ----
  final Color line;
  final Color lineStrong;
  final Color canvas;
  final Color canvasStrong;
  final Color surface;

  // ---- 语义色 ----
  final Color danger50;
  final Color danger600;
  final Color danger700;
  final Color success50;
  final Color success600;
  final Color success700;
  final Color warning50;
  final Color warning600;

  // ---- 文件类型 ----
  final Color fileFolder;
  final Color fileImage;
  final Color fileVideo;
  final Color filePdf;
  final Color fileDoc;
  final Color fileSheet;
  final Color fileSlides;
  final Color fileArchive;
  final Color fileOther;

  // ---- 圆角 ----
  final double radiusCard;
  final double radiusControl;

  // ---- 间距（4px 基数）----
  final double space1;
  final double space2;
  final double space3;
  final double space4;
  final double space5;
  final double space6;
  final double space8;

  // ---- 字号 ----
  final double textXs;
  final double textSm;
  final double textMd;
  final double textLg;
  final double textXl;
  final double text2xl;

  // ---- 阴影与动效 ----
  final List<BoxShadow> shadowPop;
  final List<BoxShadow> shadowWindow;
  final Cubic easeOut;
  final Duration durationFast;
  final Duration durationBase;
  final Duration durationSlow;

  /// 按系统主题选择亮/暗令牌。
  static DesktopTokens of(BuildContext context) {
    return Theme.of(context).brightness == Brightness.dark ? dark : light;
  }

  /// 亮色令牌：数值必须与 `design/desktop/tokens.css` 一致。
  static const DesktopTokens light = DesktopTokens(
    brightness: Brightness.light,
    brand50: Color(0xFFEEF4FF),
    brand100: Color(0xFFDBE7FF),
    brand600: Color(0xFF0061FE),
    brand700: Color(0xFF0052D9),
    ink900: Color(0xFF1E1919),
    ink600: Color(0xFF52525B),
    ink500: Color(0xFF71717A),
    ink400: Color(0xFFA1A1AA),
    line: Color(0xFFE7E5E4),
    lineStrong: Color(0xFFD6D3D1),
    canvas: Color(0xFFF7F7F6),
    canvasStrong: Color(0xFFEFEEEC),
    surface: Color(0xFFFFFFFF),
    danger50: Color(0xFFFEF2F2),
    danger600: Color(0xFFDC2626),
    danger700: Color(0xFFB91C1C),
    success50: Color(0xFFECFDF5),
    success600: Color(0xFF059669),
    success700: Color(0xFF047857),
    warning50: Color(0xFFFFFBEB),
    warning600: Color(0xFFD97706),
    fileFolder: Color(0xFF0061FE),
    fileImage: Color(0xFF059669),
    fileVideo: Color(0xFF7C3AED),
    filePdf: Color(0xFFDC2626),
    fileDoc: Color(0xFF2563EB),
    fileSheet: Color(0xFF16A34A),
    fileSlides: Color(0xFFEA580C),
    fileArchive: Color(0xFFA16207),
    fileOther: Color(0xFF71717A),
    radiusCard: 12,
    radiusControl: 8,
    space1: 4,
    space2: 8,
    space3: 12,
    space4: 16,
    space5: 20,
    space6: 24,
    space8: 32,
    textXs: 12,
    textSm: 13,
    textMd: 14,
    textLg: 16,
    textXl: 20,
    text2xl: 24,
    shadowPop: [
      BoxShadow(color: Color(0x1F000000), blurRadius: 24, offset: Offset(0, 8)),
    ],
    shadowWindow: [
      BoxShadow(color: Color(0x14000000), spreadRadius: 1),
      BoxShadow(
        color: Color(0x2E000000),
        blurRadius: 60,
        offset: Offset(0, 24),
      ),
    ],
    easeOut: Cubic(0.2, 0, 0, 1),
    durationFast: Duration(milliseconds: 120),
    durationBase: Duration(milliseconds: 180),
    durationSlow: Duration(milliseconds: 260),
  );

  /// 暗色对照色板（tokens.css 尚未定义暗色，见类注释）。
  static const DesktopTokens dark = DesktopTokens(
    brightness: Brightness.dark,
    brand50: Color(0xFF15243D),
    brand100: Color(0xFF1D3357),
    brand600: Color(0xFF4D8DFF),
    brand700: Color(0xFF7DB0FF),
    ink900: Color(0xFFF4F4F5),
    ink600: Color(0xFFC4C4CC),
    ink500: Color(0xFFA1A1AA),
    ink400: Color(0xFF71717A),
    line: Color(0xFF2A2A31),
    lineStrong: Color(0xFF3F3F46),
    canvas: Color(0xFF18181B),
    canvasStrong: Color(0xFF27272A),
    surface: Color(0xFF1F1F23),
    danger50: Color(0xFF2A1517),
    danger600: Color(0xFFF87171),
    danger700: Color(0xFFFCA5A5),
    success50: Color(0xFF10201A),
    success600: Color(0xFF34D399),
    success700: Color(0xFF6EE7B7),
    warning50: Color(0xFF2A1F10),
    warning600: Color(0xFFFBBF24),
    fileFolder: Color(0xFF4D8DFF),
    fileImage: Color(0xFF34D399),
    fileVideo: Color(0xFFA78BFA),
    filePdf: Color(0xFFF87171),
    fileDoc: Color(0xFF60A5FA),
    fileSheet: Color(0xFF4ADE80),
    fileSlides: Color(0xFFFB923C),
    fileArchive: Color(0xFFFACC15),
    fileOther: Color(0xFFA1A1AA),
    radiusCard: 12,
    radiusControl: 8,
    space1: 4,
    space2: 8,
    space3: 12,
    space4: 16,
    space5: 20,
    space6: 24,
    space8: 32,
    textXs: 12,
    textSm: 13,
    textMd: 14,
    textLg: 16,
    textXl: 20,
    text2xl: 24,
    shadowPop: [
      BoxShadow(color: Color(0x66000000), blurRadius: 24, offset: Offset(0, 8)),
    ],
    shadowWindow: [
      BoxShadow(color: Color(0x66000000), spreadRadius: 1),
      BoxShadow(
        color: Color(0x99000000),
        blurRadius: 60,
        offset: Offset(0, 24),
      ),
    ],
    easeOut: Cubic(0.2, 0, 0, 1),
    durationFast: Duration(milliseconds: 120),
    durationBase: Duration(milliseconds: 180),
    durationSlow: Duration(milliseconds: 260),
  );
}
