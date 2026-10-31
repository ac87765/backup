import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

/// Dart 侧桥接边界测试（design D2，tasks 4.2）。
///
/// 只扫描生成文件中“公开返回类型 / 事件类型与 DTO 字段”，不扫描
/// `frb_generated*.dart` 的内部编解码辅助函数（它们为通用 codec 生成
/// `Uint8List`，与桥接 DTO 无关）。新增 DTO 或 API 时会自动被检查。
void main() {
  test('生成的 API 返回类型 / 事件类型与 DTO 字段不含字节数组或敏感名称', () {
    final directory = Directory('lib/src/rust/api');
    expect(
      directory.existsSync(),
      isTrue,
      reason: '请先运行 flutter_rust_bridge_codegen generate',
    );
    final files =
        directory
            .listSync(recursive: true)
            .whereType<File>()
            .where((file) => file.path.endsWith('.dart'))
            .toList()
          ..sort((left, right) => left.path.compareTo(right.path));
    expect(files.length, greaterThanOrEqualTo(7), reason: '应扫描全部生成的 API 文件');

    var sawAccountDto = false;
    final violations = <String>[];
    for (final file in files) {
      final source = file.readAsStringSync();
      if (source.contains('class AccountDto')) {
        sawAccountDto = true;
      }
      violations.addAll(checkDartSource(source, file: file.path));
    }
    expect(sawAccountDto, isTrue, reason: '没有找到 AccountDto，说明扫描范围失效');
    expect(violations, isEmpty, reason: violations.join('\n'));
  });

  test('检查器对含 Uint8List 与敏感字段的示例会报错（反例自检）', () {
    const leaky = '''
Future<Uint8List> downloadBytes() => throw UnimplementedError();

class LeakyDto {
  final Uint8List data;
  final String authToken;
}
''';
    final violations = checkDartSource(leaky, file: 'leaky.dart');
    expect(
      violations.length,
      greaterThanOrEqualTo(3),
      reason: violations.join('\n'),
    );
    expect(violations.any((message) => message.contains('Uint8List')), isTrue);
    expect(violations.any((message) => message.contains('authToken')), isTrue);
  });
}

/// 返回人类可读的问题列表；空列表表示通过。
List<String> checkDartSource(String source, {required String file}) {
  final violations = <String>[];
  final code = _stripComments(source);

  for (final match in _byteTypePattern.allMatches(code)) {
    violations.add('$file: 出现字节数组类型 ${match.group(1)}');
  }

  for (final match in _fieldPattern.allMatches(code)) {
    final name = match.group(1);
    if (name != null && _isSensitiveName(name)) {
      violations.add('$file: 字段 `$name` 命中敏感名称检查');
    }
  }

  for (final match in _returnTypePattern.allMatches(code)) {
    final returnType = match.group(1) ?? '';
    for (final byte in _byteTypePattern.allMatches(returnType)) {
      violations.add('$file: 返回/事件类型含有 ${byte.group(1)}');
    }
  }
  return violations;
}

final RegExp _byteTypePattern = RegExp(
  r'\b(Uint8List|Int8List|ByteData|ByteBuffer|ZeroCopyBuffer|List<int>)\b',
);

/// 生成类的字段声明：`final String? name;`。
final RegExp _fieldPattern = RegExp(
  r'\bfinal\s+[A-Za-z0-9_<>?,\s.]+\s+([A-Za-z_]\w*)\s*;',
);

/// 生成函数的返回类型：`Future<T> name(` / `Stream<T> name(`。
final RegExp _returnTypePattern = RegExp(
  r'\b(?:Future|Stream)<([^;]+?)>\s+[A-Za-z_]\w*\s*\(',
);

bool _isSensitiveName(String name) {
  final lower = name.toLowerCase();
  return lower.contains('key') ||
      lower.contains('secret') ||
      lower.contains('token') ||
      lower.contains('password') ||
      lower.contains('auth');
}

String _stripComments(String source) {
  final withoutBlocks = source.replaceAll(RegExp(r'/\*[\s\S]*?\*/'), '');
  return withoutBlocks
      .split('\n')
      .where((line) => !line.trimLeft().startsWith('//'))
      .join('\n');
}
