import 'dart:io';

/// 从系统拖入的单个路径分类。
enum DroppedEntryKind { file, folder, missing }

DroppedEntryKind classifyDroppedPath(
  String path, {
  bool Function(String path)? isDirectory,
  bool Function(String path)? isFile,
}) {
  final directoryCheck =
      isDirectory ?? (value) => FileSystemEntity.isDirectorySync(value);
  if (directoryCheck(path)) {
    return DroppedEntryKind.folder;
  }
  final fileCheck = isFile ?? (value) => File(value).existsSync();
  if (fileCheck(path)) {
    return DroppedEntryKind.file;
  }
  return DroppedEntryKind.missing;
}

class DropPlan {
  const DropPlan({
    required this.files,
    required this.folders,
    required this.missing,
  });

  final List<String> files;
  final List<String> folders;
  final List<String> missing;

  bool get isEmpty => files.isEmpty && folders.isEmpty;
}

DropPlan planDropPaths(
  Iterable<String> paths, {
  bool Function(String path)? isDirectory,
  bool Function(String path)? isFile,
}) {
  final files = <String>[];
  final folders = <String>[];
  final missing = <String>[];
  for (final path in paths) {
    switch (classifyDroppedPath(
      path,
      isDirectory: isDirectory,
      isFile: isFile,
    )) {
      case DroppedEntryKind.file:
        files.add(path);
      case DroppedEntryKind.folder:
        folders.add(path);
      case DroppedEntryKind.missing:
        missing.add(path);
    }
  }
  return DropPlan(files: files, folders: folders, missing: missing);
}

/// 拖拽上传分流：文件走“上传文件”，文件夹走“上传文件夹”。抽成纯函数 +
/// 回调，组件测试可直接验证分流，不依赖平台拖拽通道。
Future<void> handleDroppedPaths({
  required Iterable<String> paths,
  required Future<void> Function(List<String> files) uploadFiles,
  required Future<void> Function(String folder) uploadFolder,
  bool Function(String path)? isDirectory,
  bool Function(String path)? isFile,
}) async {
  final plan = planDropPaths(paths, isDirectory: isDirectory, isFile: isFile);
  if (plan.files.isNotEmpty) {
    await uploadFiles(plan.files);
  }
  for (final folder in plan.folders) {
    await uploadFolder(folder);
  }
}
