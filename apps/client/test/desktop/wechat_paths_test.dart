import 'package:encrypted_backup_client/desktop/state/wechat_paths.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  const root = r'C:\Users\tlj\Documents\xwechat_files';

  test('大小写与分隔符不影响“位于微信数据目录内”的判断', () {
    expect(
      isPathInsideWechatData(
        r'c:/users/TLJ/documents/XWECHAT_FILES/wxid_a/msg/file',
        const <String>[root],
      ),
      isTrue,
    );
    expect(
      isPathInsideWechatData(r'C:\Users\tlj\Documents', const <String>[root]),
      isFalse,
    );
    expect(
      isPathInsideWechatData(
        r'C:\Users\tlj\Documents\xwechat_files2',
        const <String>[root],
      ),
      isFalse,
    );
  });

  test('祖先文件夹包含微信数据根', () {
    expect(
      containsWechatDataRoot(r'C:\Users\tlj\Documents', const <String>[root]),
      isTrue,
    );
    expect(containsWechatDataRoot(root, const <String>[root]), isFalse);
    expect(
      containsWechatDataRoot(
        r'C:\Users\tlj\Documents\xwechat_files\wxid_a',
        const <String>[root],
      ),
      isFalse,
    );
  });
}
