import 'dart:convert';

/// 文件/文件夹名称的本地校验（服务端只看到名称令牌，无法做这些检查）。
///
/// 返回中文错误文案；合法时返回 null。
String? validateNodeName(String name) {
  if (name.isEmpty) {
    return '名称不能为空';
  }
  if (name.trim().isEmpty) {
    return '名称不能只包含空白字符';
  }
  if (name.contains('/') || name.contains('\\')) {
    return '名称不能包含斜杠';
  }
  if (name.contains('\u0000')) {
    return '名称不能包含 NUL 字符';
  }
  if (name == '.' || name == '..') {
    return '名称不能是 "." 或 ".."';
  }
  if (utf8.encode(name).length > 255) {
    return '名称不能超过 255 个 UTF-8 字节';
  }
  return null;
}
