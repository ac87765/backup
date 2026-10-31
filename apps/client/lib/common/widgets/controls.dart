import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../../core/theme/design_tokens.dart';

// ---------------------------------------------------------------------------
// 按钮
// ---------------------------------------------------------------------------

enum TokensButtonVariant { secondary, primary, ghost, danger }

class TokensButton extends StatefulWidget {
  const TokensButton({
    required this.label,
    required this.onPressed,
    this.variant = TokensButtonVariant.secondary,
    this.icon,
    this.trailingIcon,
    this.large = false,
    this.block = false,
    this.busy = false,
    this.progress = false,
    super.key,
  });

  final String label;
  final VoidCallback? onPressed;
  final TokensButtonVariant variant;
  final IconData? icon;
  final IconData? trailingIcon;
  final bool large;
  final bool block;
  final bool busy;

  /// 处理中在按钮底部循环扫过的 2px 进度条（login.html `.btn-progress`）。
  /// “减少动态效果”时不绘制。
  final bool progress;

  @override
  State<TokensButton> createState() => _TokensButtonState();
}

class _TokensButtonState extends State<TokensButton>
    with SingleTickerProviderStateMixin {
  bool _hover = false;
  bool _reduce = false;

  late final AnimationController _progressController = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 1100),
  );

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _reduce = MediaQuery.disableAnimationsOf(context);
    _syncProgress();
  }

  @override
  void didUpdateWidget(TokensButton oldWidget) {
    super.didUpdateWidget(oldWidget);
    _syncProgress();
  }

  void _syncProgress() {
    if (widget.progress && !_reduce) {
      if (!_progressController.isAnimating) {
        _progressController.repeat();
      }
    } else {
      _progressController.stop();
      _progressController.value = 0;
    }
  }

  @override
  void dispose() {
    _progressController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final enabled = widget.onPressed != null && !widget.busy;
    final background = switch (widget.variant) {
      TokensButtonVariant.primary =>
        _hover && enabled ? tokens.brand700 : tokens.brand600,
      TokensButtonVariant.secondary =>
        _hover && enabled ? tokens.canvas : tokens.surface,
      TokensButtonVariant.ghost =>
        _hover && enabled ? tokens.canvasStrong : Colors.transparent,
      TokensButtonVariant.danger =>
        _hover && enabled ? tokens.danger700 : tokens.danger600,
    };
    final foreground = switch (widget.variant) {
      TokensButtonVariant.primary ||
      TokensButtonVariant.danger => tokens.onBrand,
      TokensButtonVariant.ghost => tokens.ink600,
      TokensButtonVariant.secondary => tokens.ink900,
    };
    final borderColor = switch (widget.variant) {
      TokensButtonVariant.primary => tokens.brand600,
      TokensButtonVariant.danger => tokens.danger600,
      TokensButtonVariant.ghost => Colors.transparent,
      TokensButtonVariant.secondary => tokens.lineStrong,
    };
    final height = widget.large
        ? DesktopMetrics.buttonHeightLarge
        : DesktopMetrics.buttonHeight;
    return MouseRegion(
      cursor: enabled ? SystemMouseCursors.click : SystemMouseCursors.basic,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: Opacity(
        // 设计稿：处理中（login.html working）0.8；不可用（add-folder.html quota）0.45。
        opacity: enabled
            ? 1.0
            : widget.busy
            ? DesktopMetrics.busyOpacity
            : DesktopMetrics.disabledOpacity,
        child: SizedBox(
          width: widget.block ? double.infinity : null,
          height: height,
          // passthrough：按钮本身按原来的约束布局（非块级按钮在 Row 中宽度
          // 不受限）；expand 会把无限宽度传给 Material 导致布局错误。
          child: Stack(
            fit: StackFit.passthrough,
            children: [
              Material(
                color: background,
                shape: RoundedRectangleBorder(
                  borderRadius: BorderRadius.circular(tokens.radiusControl),
                  side: BorderSide(color: borderColor),
                ),
                child: InkWell(
                  borderRadius: BorderRadius.circular(tokens.radiusControl),
                  onTap: enabled ? widget.onPressed : null,
                  child: Padding(
                    padding: EdgeInsets.symmetric(
                      horizontal: widget.large ? 18 : tokens.space3,
                    ),
                    child: Row(
                      mainAxisSize: MainAxisSize.min,
                      mainAxisAlignment: MainAxisAlignment.center,
                      children: [
                        if (widget.icon != null) ...[
                          Icon(widget.icon!, size: 16, color: foreground),
                          SizedBox(width: tokens.space1 + 2),
                        ],
                        Text(
                          widget.label,
                          style: TextStyle(
                            color: foreground,
                            fontSize: widget.large
                                ? tokens.textMd
                                : tokens.textSm,
                            fontWeight: FontWeight.w500,
                          ),
                        ),
                        if (widget.trailingIcon != null) ...[
                          SizedBox(width: tokens.space1 + 2),
                          Icon(
                            widget.trailingIcon!,
                            size: 14,
                            color: foreground,
                          ),
                        ],
                      ],
                    ),
                  ),
                ),
              ),
              if (widget.progress && !_reduce) _progressBar(tokens),
            ],
          ),
        ),
      ),
    );
  }

  /// 底部 2px 细条从 -40% 扫到 100%（login.html `@keyframes sweep`，1.1s
  /// ease-in-out）。减少动态效果时整个条不绘制。
  Widget _progressBar(DesktopTokens tokens) {
    return Positioned(
      key: const Key('button-progress'),
      left: 0,
      right: 0,
      bottom: 0,
      height: 2,
      child: ClipRRect(
        borderRadius: BorderRadius.vertical(
          bottom: Radius.circular(tokens.radiusControl),
        ),
        child: LayoutBuilder(
          builder: (context, constraints) {
            final width = constraints.maxWidth * 0.4;
            return AnimatedBuilder(
              animation: _progressController,
              builder: (context, child) {
                final t = Curves.easeInOut.transform(_progressController.value);
                return Stack(
                  fit: StackFit.expand,
                  children: [
                    Positioned(
                      left: -width + (constraints.maxWidth + width) * t,
                      top: 0,
                      bottom: 0,
                      width: width,
                      child: ColoredBox(
                        color: tokens.onBrand.withValues(alpha: 0.85),
                      ),
                    ),
                  ],
                );
              },
            );
          },
        ),
      ),
    );
  }
}

class TokensIconButton extends StatefulWidget {
  const TokensIconButton({
    required this.icon,
    required this.onPressed,
    this.tooltip,
    this.size = DesktopMetrics.buttonIconSize,
    this.color,
    this.ghost = true,
    super.key,
  });

  final IconData icon;
  final VoidCallback? onPressed;
  final String? tooltip;
  final double size;
  final Color? color;
  final bool ghost;

  @override
  State<TokensIconButton> createState() => _TokensIconButtonState();
}

class _TokensIconButtonState extends State<TokensIconButton> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final enabled = widget.onPressed != null;
    final button = MouseRegion(
      cursor: enabled ? SystemMouseCursors.click : SystemMouseCursors.basic,
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: SizedBox(
        width: widget.size,
        height: widget.size,
        child: Material(
          color: _hover && enabled && widget.ghost
              ? tokens.canvasStrong
              : Colors.transparent,
          borderRadius: BorderRadius.circular(tokens.radiusControl),
          child: InkWell(
            borderRadius: BorderRadius.circular(tokens.radiusControl),
            onTap: enabled ? widget.onPressed : null,
            child: Icon(
              widget.icon,
              size: 16,
              color: widget.color ?? tokens.ink500,
            ),
          ),
        ),
      ),
    );
    if (widget.tooltip == null) {
      return button;
    }
    return Tooltip(message: widget.tooltip!, child: button);
  }
}

// ---------------------------------------------------------------------------
// 输入框
// ---------------------------------------------------------------------------

class AppTextField extends StatefulWidget {
  const AppTextField({
    required this.controller,
    this.hint,
    this.obscure = false,
    this.enabled = true,
    this.readOnly = false,
    this.error = false,
    this.small = false,
    this.prefixIcon,
    this.autofocus = false,
    this.onChanged,
    this.onSubmitted,
    this.autofillHints,
    this.textInputAction,
    super.key,
  });

  final TextEditingController controller;
  final String? hint;
  final bool obscure;
  final bool enabled;

  /// 只读但保持正常外观（processing 时邮箱输入框仍为白底）。
  final bool readOnly;
  final bool error;
  final bool small;
  final IconData? prefixIcon;
  final bool autofocus;
  final ValueChanged<String>? onChanged;
  final ValueChanged<String>? onSubmitted;
  final Iterable<String>? autofillHints;
  final TextInputAction? textInputAction;

  @override
  State<AppTextField> createState() => _AppTextFieldState();
}

class _AppTextFieldState extends State<AppTextField> {
  final FocusNode _focus = FocusNode();
  bool _focused = false;

  @override
  void initState() {
    super.initState();
    _focus.addListener(_onFocusChanged);
  }

  void _onFocusChanged() {
    if (_focused != _focus.hasFocus) {
      setState(() => _focused = _focus.hasFocus);
    }
  }

  @override
  void dispose() {
    _focus.removeListener(_onFocusChanged);
    _focus.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final borderColor = widget.error
        ? tokens.danger600
        : _focused
        ? tokens.brand600
        : tokens.lineStrong;
    final height = widget.small
        ? DesktopMetrics.inputHeightSmall
        : DesktopMetrics.inputHeight;
    return Container(
      height: height,
      padding: EdgeInsets.symmetric(horizontal: tokens.space3),
      decoration: BoxDecoration(
        color: widget.enabled ? tokens.surface : tokens.canvas,
        borderRadius: BorderRadius.circular(tokens.radiusControl),
        border: Border.all(color: borderColor),
        boxShadow: _focused && !widget.error
            ? <BoxShadow>[BoxShadow(color: tokens.brand100, spreadRadius: 3)]
            : null,
      ),
      child: Row(
        children: [
          if (widget.prefixIcon != null) ...[
            Icon(widget.prefixIcon!, size: 16, color: tokens.ink400),
            SizedBox(width: tokens.space2),
          ],
          Expanded(
            child: TextField(
              controller: widget.controller,
              focusNode: _focus,
              enabled: widget.enabled,
              readOnly: widget.readOnly,
              obscureText: widget.obscure,
              obscuringCharacter: '•',
              autofocus: widget.autofocus,
              enableSuggestions: false,
              autocorrect: false,
              smartDashesType: SmartDashesType.disabled,
              smartQuotesType: SmartQuotesType.disabled,
              autofillHints: widget.autofillHints,
              textInputAction: widget.textInputAction,
              textAlignVertical: TextAlignVertical.center,
              onChanged: widget.onChanged,
              onSubmitted: widget.onSubmitted,
              cursorColor: tokens.brand600,
              style: TextStyle(
                color: widget.enabled ? tokens.ink900 : tokens.ink500,
                fontSize: tokens.textSm,
              ),
              decoration: InputDecoration(
                isDense: true,
                isCollapsed: true,
                border: InputBorder.none,
                hintText: widget.hint,
                hintStyle: TextStyle(
                  color: tokens.ink400,
                  fontSize: tokens.textSm,
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

// ---------------------------------------------------------------------------
// 开关、分段控件、徽标、提示
// ---------------------------------------------------------------------------

class TokensToggle extends StatelessWidget {
  const TokensToggle({required this.value, required this.onChanged, super.key});

  final bool value;
  final ValueChanged<bool>? onChanged;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final enabled = onChanged != null;
    final reduceMotion = MediaQuery.disableAnimationsOf(context);
    return Opacity(
      opacity: enabled ? 1.0 : 0.55,
      child: GestureDetector(
        onTap: enabled ? () => onChanged!(!value) : null,
        child: MouseRegion(
          cursor: enabled ? SystemMouseCursors.click : SystemMouseCursors.basic,
          child: AnimatedContainer(
            duration: reduceMotion ? Duration.zero : tokens.durationBase,
            curve: tokens.easeOut,
            width: DesktopMetrics.toggleWidth,
            height: DesktopMetrics.toggleHeight,
            padding: const EdgeInsets.all(2),
            decoration: BoxDecoration(
              color: value ? tokens.brand600 : tokens.lineStrong,
              borderRadius: BorderRadius.circular(10),
            ),
            child: AnimatedAlign(
              duration: reduceMotion ? Duration.zero : tokens.durationBase,
              curve: tokens.easeOut,
              alignment: value ? Alignment.centerRight : Alignment.centerLeft,
              child: Container(
                width: DesktopMetrics.toggleKnob,
                height: DesktopMetrics.toggleKnob,
                decoration: BoxDecoration(
                  color: tokens.onBrand,
                  shape: BoxShape.circle,
                  boxShadow: <BoxShadow>[
                    BoxShadow(
                      color: tokens.knobShadow,
                      blurRadius: 2,
                      offset: const Offset(0, 1),
                    ),
                  ],
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class SegmentedOption<T> {
  const SegmentedOption({
    required this.value,
    required this.icon,
    required this.tooltip,
  });

  final T value;
  final IconData icon;
  final String tooltip;
}

class TokensSegmented<T> extends StatelessWidget {
  const TokensSegmented({
    required this.options,
    required this.value,
    required this.onChanged,
    super.key,
  });

  final List<SegmentedOption<T>> options;
  final T value;
  final ValueChanged<T> onChanged;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return Container(
      padding: const EdgeInsets.all(2),
      decoration: BoxDecoration(
        border: Border.all(color: tokens.lineStrong),
        borderRadius: BorderRadius.circular(tokens.radiusControl),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (final option in options)
            Tooltip(
              message: option.tooltip,
              child: Material(
                color: option.value == value
                    ? tokens.canvasStrong
                    : Colors.transparent,
                borderRadius: BorderRadius.circular(6),
                child: InkWell(
                  borderRadius: BorderRadius.circular(6),
                  onTap: () => onChanged(option.value),
                  child: SizedBox(
                    width: DesktopMetrics.segmentedCellWidth,
                    height: DesktopMetrics.segmentedCellHeight,
                    child: Icon(
                      option.icon,
                      size: 14,
                      color: option.value == value
                          ? tokens.ink900
                          : tokens.ink500,
                    ),
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

enum PillTone { ok, busy, paused, warn, error }

class TokensPill extends StatelessWidget {
  const TokensPill({
    required this.label,
    this.tone = PillTone.ok,
    this.icon,
    super.key,
  });

  final String label;
  final PillTone tone;
  final IconData? icon;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final (background, foreground) = switch (tone) {
      PillTone.ok => (tokens.success50, tokens.success700),
      PillTone.busy => (tokens.brand50, tokens.brand700),
      PillTone.paused => (tokens.canvasStrong, tokens.ink600),
      PillTone.warn => (tokens.warning50, tokens.warning600),
      PillTone.error => (tokens.danger50, tokens.danger700),
    };
    return Container(
      height: 22,
      padding: EdgeInsets.symmetric(horizontal: tokens.space2),
      decoration: BoxDecoration(
        color: background,
        borderRadius: BorderRadius.circular(11),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (icon != null) ...[
            Icon(icon!, size: 12, color: foreground),
            const SizedBox(width: 5),
          ],
          Text(
            label,
            style: TextStyle(
              color: foreground,
              fontSize: tokens.textXs,
              fontWeight: FontWeight.w500,
            ),
          ),
        ],
      ),
    );
  }
}

enum NoteTone { plain, info, warn }

class TokensNote extends StatelessWidget {
  const TokensNote({
    required this.text,
    this.icon = LucideIcons.info,
    this.tone = NoteTone.plain,
    super.key,
  });

  final String text;
  final IconData icon;
  final NoteTone tone;

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final (background, foreground) = switch (tone) {
      NoteTone.plain => (tokens.canvas, tokens.ink600),
      NoteTone.info => (tokens.brand50, tokens.brand700),
      NoteTone.warn => (tokens.warning50, tokens.warningTextStrong),
    };
    return Container(
      padding: EdgeInsets.symmetric(
        horizontal: tokens.space3 + 2,
        vertical: tokens.space3,
      ),
      decoration: BoxDecoration(
        color: background,
        borderRadius: BorderRadius.circular(tokens.radiusControl),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.only(top: 1),
            child: Icon(icon, size: 16, color: foreground),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Text(
              text,
              style: TextStyle(
                color: foreground,
                fontSize: tokens.textXs,
                height: 1.45,
              ),
            ),
          ),
        ],
      ),
    );
  }
}
