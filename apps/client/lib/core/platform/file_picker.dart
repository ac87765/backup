import 'package:file_selector/file_selector.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// 文件/目录选择（file_selector）。抽成接口便于组件测试注入假实现。
abstract class FilePicker {
  /// 多选上传文件；取消时返回空列表。
  Future<List<String>> pickFiles();

  /// 选择保存位置；取消时返回 null。
  Future<String?> pickSaveLocation(String suggestedName);
}

/// 生产实现：调用系统文件选择器。
class PlatformFilePicker implements FilePicker {
  const PlatformFilePicker();

  @override
  Future<List<String>> pickFiles() async {
    final files = await openFiles();
    return files.map((file) => file.path).toList(growable: false);
  }

  @override
  Future<String?> pickSaveLocation(String suggestedName) async {
    final location = await getSaveLocation(suggestedName: suggestedName);
    return location?.path;
  }
}

/// 文件选择器入口；测试中 override 成假实现，避免触达平台通道。
final filePickerProvider = Provider<FilePicker>((ref) {
  return const PlatformFilePicker();
});
