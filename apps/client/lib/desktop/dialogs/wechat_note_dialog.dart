import 'package:flutter/material.dart';

import '../../common/widgets/app_dialog.dart';
import '../../common/widgets/controls.dart';
import '../../core/theme/design_tokens.dart';

sealed class WechatNoteResult {
  const WechatNoteResult();
}

class WechatNoteSave extends WechatNoteResult {
  const WechatNoteSave(this.note);

  final String note;
}

class WechatNoteClear extends WechatNoteResult {
  const WechatNoteClear();
}

/// 设置 / 清除微信账号备注名（只保存在本机状态库）。
Future<WechatNoteResult?> showWechatNoteDialog({
  required BuildContext context,
  required String accountName,
  required String? initialNote,
}) {
  return showTokensDialog<WechatNoteResult>(
    context: context,
    builder: (dialogContext) =>
        _WechatNoteDialog(accountName: accountName, initialNote: initialNote),
  );
}

class _WechatNoteDialog extends StatefulWidget {
  const _WechatNoteDialog({
    required this.accountName,
    required this.initialNote,
  });

  final String accountName;
  final String? initialNote;

  @override
  State<_WechatNoteDialog> createState() => _WechatNoteDialogState();
}

class _WechatNoteDialogState extends State<_WechatNoteDialog> {
  late final TextEditingController _controller = TextEditingController(
    text: widget.initialNote ?? '',
  );
  String? _error;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  void _save() {
    final note = _controller.text.trim();
    if (note.isEmpty) {
      setState(() => _error = '请输入备注名，或选择“清除备注名”');
      return;
    }
    Navigator.of(context).pop(WechatNoteSave(note));
  }

  @override
  Widget build(BuildContext context) {
    final tokens = DesktopTokens.of(context);
    final hasNote = widget.initialNote?.isNotEmpty == true;
    return TokensDialogShell(
      title: '设置备注名',
      width: DesktopMetrics.dialogWidthSmall,
      onClose: () => Navigator.of(context).pop(),
      body: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            '账号目录：${widget.accountName}',
            style: TextStyle(
              color: tokens.ink500,
              fontSize: tokens.textXs,
              fontFamily: DesktopMetrics.monoFamily,
              fontFamilyFallback: DesktopMetrics.monoFallback,
            ),
          ),
          SizedBox(height: tokens.space3),
          Text(
            '备注名只保存在这台电脑上，不会上传到云端。',
            style: TextStyle(color: tokens.ink500, fontSize: tokens.textXs),
          ),
          SizedBox(height: tokens.space2),
          AppTextField(
            key: const Key('wechat-note-input'),
            controller: _controller,
            hint: '例如：工作微信',
            autofocus: true,
            error: _error != null,
            onSubmitted: (_) => _save(),
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
        if (hasNote)
          TokensButton(
            key: const Key('wechat-note-clear'),
            label: '清除备注名',
            variant: TokensButtonVariant.ghost,
            onPressed: () => Navigator.of(context).pop(const WechatNoteClear()),
          ),
        TokensButton(label: '取消', onPressed: () => Navigator.of(context).pop()),
        TokensButton(
          key: const Key('wechat-note-save'),
          label: '保存',
          variant: TokensButtonVariant.primary,
          onPressed: _save,
        ),
      ],
    );
  }
}
