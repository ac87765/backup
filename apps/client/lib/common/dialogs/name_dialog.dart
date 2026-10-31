import 'package:flutter/material.dart';

import '../../core/errors/bridge_error_text.dart';
import '../../core/format/name_validation.dart';
import '../../core/theme/design_tokens.dart';
import '../widgets/app_dialog.dart';
import '../widgets/controls.dart';

/// 新建文件夹 / 重命名共用的输入对话框。
///
/// [onSubmit] 返回桥接错误文案表示失败（对话框保持打开），返回 null 表示成功。
Future<String?> showNodeNameDialog({
  required BuildContext context,
  required String title,
  required String label,
  required String submitLabel,
  String initialValue = '',
  String? placeholder,
  required Future<String?> Function(String name) onSubmit,
}) {
  return showTokensDialog<String>(
    context: context,
    builder: (dialogContext) => _NodeNameDialog(
      title: title,
      label: label,
      submitLabel: submitLabel,
      initialValue: initialValue,
      placeholder: placeholder,
      onSubmit: onSubmit,
    ),
  );
}

class _NodeNameDialog extends StatefulWidget {
  const _NodeNameDialog({
    required this.title,
    required this.label,
    required this.submitLabel,
    required this.initialValue,
    required this.onSubmit,
    this.placeholder,
  });

  final String title;
  final String label;
  final String submitLabel;
  final String initialValue;
  final String? placeholder;
  final Future<String?> Function(String name) onSubmit;

  @override
  State<_NodeNameDialog> createState() => _NodeNameDialogState();
}

class _NodeNameDialogState extends State<_NodeNameDialog> {
  late final TextEditingController _controller = TextEditingController(
    text: widget.initialValue,
  );
  String? _error;
  bool _busy = false;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  Future<void> _submit() async {
    final name = _controller.text.trim();
    final localError = validateNodeName(name);
    if (localError != null) {
      setState(() => _error = localError);
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final error = await widget.onSubmit(name);
      if (!mounted) {
        return;
      }
      if (error != null) {
        setState(() {
          _busy = false;
          _error = error;
        });
        return;
      }
      Navigator.of(context).pop(name);
    } catch (error) {
      if (!mounted) {
        return;
      }
      setState(() {
        _busy = false;
        _error = bridgeErrorText(error);
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    return TokensDialogShell(
      title: widget.title,
      width: DesktopMetrics.dialogWidthSmall,
      onClose: _busy ? () {} : () => Navigator.of(context).pop(),
      body: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            widget.label,
            style: TextStyle(
              color: tokens.ink900,
              fontSize: tokens.textSm,
              fontWeight: FontWeight.w600,
            ),
          ),
          SizedBox(height: tokens.space2),
          AppTextField(
            key: const Key('name-dialog-input'),
            controller: _controller,
            hint: widget.placeholder,
            autofocus: true,
            error: _error != null,
            enabled: !_busy,
            onSubmitted: (_) => _submit(),
          ),
          if (_error != null) ...[
            SizedBox(height: tokens.space2),
            Text(
              _error!,
              style: TextStyle(
                color: tokens.danger700,
                fontSize: tokens.textXs,
              ),
            ),
          ],
        ],
      ),
      actions: [
        TokensButton(
          label: '取消',
          onPressed: _busy ? null : () => Navigator.of(context).pop(),
        ),
        TokensButton(
          label: _busy ? '保存中…' : widget.submitLabel,
          variant: TokensButtonVariant.primary,
          onPressed: _busy ? null : _submit,
        ),
      ],
    );
  }
}
