import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';

/// 读取本地文件夹的一级子文件夹（添加备份文件夹时选择“排除子文件夹”）。
///
/// 抽成接口便于组件测试；实现不跟随符号链接，读取失败返回空列表，且不记录
/// 任何本地路径。
abstract class LocalFolderInspector {
  Future<List<String>> listSubfolders(String path);
}

class IoLocalFolderInspector implements LocalFolderInspector {
  const IoLocalFolderInspector();

  @override
  Future<List<String>> listSubfolders(String path) async {
    try {
      final entries = await Directory(path).list(followLinks: false).toList();
      final names =
          <String>[
            for (final entry in entries)
              if (entry is Directory) _name(entry.path),
          ]..sort(
            (left, right) => left.toLowerCase().compareTo(right.toLowerCase()),
          );
      return names;
    } catch (_) {
      return const <String>[];
    }
  }

  String _name(String path) {
    final normalized = path.replaceAll('\\', '/');
    final trimmed = normalized.endsWith('/')
        ? normalized.substring(0, normalized.length - 1)
        : normalized;
    final index = trimmed.lastIndexOf('/');
    return index < 0 ? trimmed : trimmed.substring(index + 1);
  }
}

final localFolderInspectorProvider = Provider<LocalFolderInspector>((ref) {
  return const IoLocalFolderInspector();
});
