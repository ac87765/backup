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
    required this.fileFolderBg,
    required this.fileDocBg,
    required this.fileSheetBg,
    required this.fileVideoBg,
    required this.fileArchiveBg,
    required this.scrim,
    required this.designCanvas,
    required this.warningTextStrong,
    required this.trafficRed,
    required this.trafficYellow,
    required this.trafficGreen,
    required this.onBrand,
    required this.knobShadow,
    required this.trafficShadow,
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

  // ---- 文件类型图标底色（tokens.css `.ficon.*` 的字面色值）----
  final Color fileFolderBg;
  final Color fileDocBg;
  final Color fileSheetBg;
  final Color fileVideoBg;
  final Color fileArchiveBg;

  // ---- 遮罩、画布与辅助色 ----
  /// 对话框遮罩：tokens.css `.scrim` 的 `rgb(30 25 25 / 0.32)`。
  final Color scrim;

  /// 设计稿窗口外画布：tokens.css `body` 的 `#dfe3e8`。
  final Color designCanvas;

  /// tokens.css `.note.warn` 的文字色 `#92400e`。
  final Color warningTextStrong;

  /// macOS 红绿灯（窗口外框图元，tokens.css `.traffic`）。
  final Color trafficRed;
  final Color trafficYellow;
  final Color trafficGreen;

  /// 品牌色底上的文字/图标（tokens.css 的 `#fff`）。
  final Color onBrand;

  /// 开关滑块阴影 `0 1px 2px rgb(0 0 0 / 0.2)`。
  final Color knobShadow;

  /// macOS 红绿灯的内描边 `rgb(0 0 0 / 0.15)`。
  final Color trafficShadow;

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
    fileFolderBg: Color(0xFFEEF4FF),
    fileDocBg: Color(0xFFEFF6FF),
    fileSheetBg: Color(0xFFF0FDF4),
    fileVideoBg: Color(0xFFF5F3FF),
    fileArchiveBg: Color(0xFFFEFCE8),
    scrim: Color(0x521E1919),
    designCanvas: Color(0xFFDFE3E8),
    warningTextStrong: Color(0xFF92400E),
    trafficRed: Color(0xFFFF5F57),
    trafficYellow: Color(0xFFFEBC2E),
    trafficGreen: Color(0xFF28C840),
    onBrand: Color(0xFFFFFFFF),
    knobShadow: Color(0x33000000),
    trafficShadow: Color(0x26000000),
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
    fileFolderBg: Color(0xFF15243D),
    fileDocBg: Color(0xFF14243D),
    fileSheetBg: Color(0xFF10201A),
    fileVideoBg: Color(0xFF231A33),
    fileArchiveBg: Color(0xFF2A2410),
    scrim: Color(0x8C000000),
    designCanvas: Color(0xFF0E0E11),
    warningTextStrong: Color(0xFFFBBF24),
    trafficRed: Color(0xFFFF5F57),
    trafficYellow: Color(0xFFFEBC2E),
    trafficGreen: Color(0xFF28C840),
    onBrand: Color(0xFFFFFFFF),
    knobShadow: Color(0x33000000),
    trafficShadow: Color(0x26000000),
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

/// 版式尺寸令牌。
///
/// 颜色、圆角、间距、字号在 [DesktopTokens]；窗口骨架与控件的固定尺寸没有
/// 对应的 CSS 变量，但同样来自 `design/desktop/tokens.css` 与各 HTML 的
/// 源码（注释中标出出处）。集中放在这里，避免组件里出现零散魔法数字。
abstract final class DesktopMetrics {
  // 等宽字体（tokens.css `.mono`）：本地路径等。等宽字体没有中文字形，
  // macOS 没有 Cascadia，因此带回退列表。
  static const String monoFamily = 'Cascadia Mono';
  static const List<String> monoFallback = <String>[
    'Consolas',
    'Menlo',
    'Microsoft YaHei UI',
    'PingFang SC',
  ];

  // 按钮不透明度（login.html working / add-folder.html quota 的内联样式）。
  static const double busyOpacity = 0.8;
  static const double disabledOpacity = 0.45;

  // 窗口（tokens.css `.window` / `.titlebar`）。
  static const double windowRadius = 8;
  static const double windowRadiusMac = 10;
  static const double titleBarHeight = 32;
  static const double titleBarHeightMac = 28;
  static const double captionButtonWidth = 46;
  static const double trafficLightSize = 12;

  // 应用骨架（tokens.css `.sidebar` / `.toolbar` / `.statusbar`）。
  static const double sidebarWidth = 232;
  static const double sidebarPadding = 12;
  static const double navItemHeight = 34;
  static const double toolbarHeight = 56;
  static const double statusBarHeight = 32;
  static const double quotaBarHeight = 6;
  static const double brandLogoSize = 28;

  // 控件（tokens.css `.btn` / `.input` / `.segmented` / `.toggle`）。
  static const double buttonHeight = 32;
  static const double buttonHeightLarge = 40;
  static const double buttonIconSize = 32;
  static const double inputHeight = 36;
  static const double inputHeightSmall = 32;
  static const double segmentedCellWidth = 28;
  static const double segmentedCellHeight = 26;
  static const double toggleWidth = 36;
  static const double toggleHeight = 20;
  static const double toggleKnob = 16;
  static const double iconTileSize = 28;
  static const double avatarSize = 28;
  static const double avatarSizeLarge = 56;
  static const double logoSizeLarge = 48;

  // 文件列表（tokens.css `.table`）。
  static const double tableHeaderHeight = 36;
  static const double tableRowHeight = 44;
  static const double tableNameColumnPadding = 20;
  static const double tableModifiedWidth = 180;
  static const double tableSizeWidth = 110;
  static const double tableActionsWidth = 48;

  // 对话框与弹出层（tokens.css `.dialog` / `.menu` / `.popover`）。
  static const double dialogWidth = 520;
  static const double dialogWidthSmall = 440;
  static const double dialogMaxHeightInset = 48;
  static const double menuWidth = 220;
  static const double menuItemHeight = 32;

  // 传输面板（files.html `.transfers`）。
  static const double transferPanelWidth = 380;
  static const double transferPanelBottom = 44;

  // 认证窗口（login.html：880×560，左侧 400 品牌面板 + 右侧表单）。
  static const double authWindowWidth = 880;
  static const double authWindowHeight = 560;
  static const double authBrandWidth = 400;
  static const double authBrandPadding = 40;
  static const double authBrandPaddingBottom = 32;
  static const double authPanePaddingTop = 40;
  static const double authPanePaddingHorizontal = 56;
  static const double authPanePaddingBottom = 28;
  static const double authFormGap = 14;

  // 设置窗口（settings.html）。
  static const double settingsWindowWidth = 760;
  static const double settingsWindowHeight = 600;
  static const double settingsTabsWidth = 188;

  // 首次引导窗口（onboarding.html）。
  static const double onboardingWindowWidth = 760;
  static const double onboardingWindowHeight = 720;

  // 主窗口（D8：1280×800，最小 960×600）。
  static const double mainWindowWidth = 1280;
  static const double mainWindowHeight = 800;
  static const double mainWindowMinWidth = 960;
  static const double mainWindowMinHeight = 600;

  // 添加备份文件夹对话框（add-folder.html，宽 560）。
  static const double backupFolderDialogWidth = 560;

  /// 文件列表每页显示数量。桥接 `list_folder` 目前一次返回全部（client-core
  /// 内部已聚合服务端分页），界面按这个大小做本地分页加载。
  static const int filePageSize = 50;
}
