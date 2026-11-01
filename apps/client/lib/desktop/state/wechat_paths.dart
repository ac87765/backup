/// 普通备份文件夹与微信数据目录的纯路径比较。
///
/// Rust 引擎在扫描普通备份文件夹时会自动排除 D1 发现的微信数据根（design D2，
/// 2026-11-01 审查者决定）。界面用同一组“数据根”做两件事：
/// - 判断普通文件夹整个位于微信数据根内部，状态显示“由微信备份处理”；
/// - 判断普通文件夹是微信数据根的祖先（需要提示已跳过多少微信数据）。
///
/// 只做展示判断，不读取目录。Windows/macOS 路径大小写不敏感；分隔符统一成
/// `/`，并去掉末尾分隔符。
library;

String comparableWechatPath(String path) {
  var value = path.replaceAll('\\', '/');
  while (value.length > 1 && value.endsWith('/')) {
    value = value.substring(0, value.length - 1);
  }
  return value.toLowerCase();
}

/// [path] 是否等于 [root] 或位于 [root] 内部。
bool wechatPathWithin(String path, String root) {
  final normalizedPath = comparableWechatPath(path);
  final normalizedRoot = comparableWechatPath(root);
  if (normalizedPath.isEmpty || normalizedRoot.isEmpty) {
    return false;
  }
  if (normalizedPath == normalizedRoot) {
    return true;
  }
  return normalizedPath.startsWith('$normalizedRoot/');
}

/// 普通备份文件夹整个位于任一微信数据根内部。
bool isPathInsideWechatData(String path, Iterable<String> dataRoots) {
  for (final root in dataRoots) {
    if (wechatPathWithin(path, root)) {
      return true;
    }
  }
  return false;
}

/// 普通备份文件夹内部包含微信数据根（可能是祖先目录，例如“文档”）。
///
/// 相等或位于数据根内部不算“包含”：前者由微信备份处理，后者不会发生嵌套
/// 排除提示。
bool containsWechatDataRoot(String path, Iterable<String> dataRoots) {
  final normalizedPath = comparableWechatPath(path);
  if (normalizedPath.isEmpty) {
    return false;
  }
  for (final root in dataRoots) {
    final normalizedRoot = comparableWechatPath(root);
    if (normalizedRoot.isEmpty || normalizedRoot == normalizedPath) {
      continue;
    }
    if (wechatPathWithin(normalizedRoot, normalizedPath)) {
      return true;
    }
  }
  return false;
}
