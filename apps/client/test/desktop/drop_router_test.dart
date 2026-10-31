import 'package:encrypted_backup_client/desktop/transfer/drop_router.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('拖入路径按文件 / 文件夹 / 不存在分流', () {
    final plan = planDropPaths(
      <String>[r'C:\照片\a.jpg', r'C:\照片', r'C:\丢失.txt'],
      isDirectory: (path) => path == r'C:\照片',
      isFile: (path) => path == r'C:\照片\a.jpg',
    );
    expect(plan.files, <String>[r'C:\照片\a.jpg']);
    expect(plan.folders, <String>[r'C:\照片']);
    expect(plan.missing, <String>[r'C:\丢失.txt']);
  });

  test('处理函数分别调用上传文件与上传文件夹', () async {
    final files = <List<String>>[];
    final folders = <String>[];
    await handleDroppedPaths(
      paths: <String>[r'C:\a.pdf', r'C:\b.jpg', r'C:\目录', r'D:\missing'],
      uploadFiles: (value) async => files.add(value),
      uploadFolder: (value) async => folders.add(value),
      isDirectory: (path) => path == r'C:\目录',
      isFile: (path) => path == r'C:\a.pdf' || path == r'C:\b.jpg',
    );
    expect(files, <List<String>>[
      <String>[r'C:\a.pdf', r'C:\b.jpg'],
    ]);
    expect(folders, <String>[r'C:\目录']);
  });

  test('单个文件走上传文件，单个文件夹走上传文件夹', () async {
    var fileCalls = 0;
    var folderCalls = 0;
    await handleDroppedPaths(
      paths: <String>[r'C:\目录'],
      uploadFiles: (value) async => fileCalls += 1,
      uploadFolder: (value) async => folderCalls += 1,
      isDirectory: (path) => true,
      isFile: (path) => false,
    );
    expect(fileCalls, 0);
    expect(folderCalls, 1);
  });
}
