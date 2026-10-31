import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';

enum StandardFolderKind { desktop, documents, pictures }

/// 首次引导里可一键选择的常见文件夹。
class StandardFolder {
  const StandardFolder({
    required this.name,
    required this.path,
    required this.kind,
  });

  final String name;
  final String path;
  final StandardFolderKind kind;
}

/// 只返回真实存在的目录；不跟随用户自定义位置（引导页可另行“添加其他文件夹”）。
List<StandardFolder> discoverStandardFolders({
  Map<String, String>? environment,
  bool Function(String path)? isDirectory,
}) {
  final env = environment ?? Platform.environment;
  final home = env['USERPROFILE'] ?? env['HOME'];
  if (home == null || home.isEmpty) {
    return const <StandardFolder>[];
  }
  final separator = Platform.pathSeparator;
  final exists = isDirectory ?? (path) => Directory(path).existsSync();
  final candidates = <StandardFolder>[
    StandardFolder(
      name: '桌面',
      path: '$home${separator}Desktop',
      kind: StandardFolderKind.desktop,
    ),
    StandardFolder(
      name: '文档',
      path: '$home${separator}Documents',
      kind: StandardFolderKind.documents,
    ),
    StandardFolder(
      name: '图片',
      path: '$home${separator}Pictures',
      kind: StandardFolderKind.pictures,
    ),
  ];
  return candidates
      .where((folder) => exists(folder.path))
      .toList(growable: false);
}

final standardFoldersProvider = FutureProvider<List<StandardFolder>>(
  (ref) async => discoverStandardFolders(),
);
