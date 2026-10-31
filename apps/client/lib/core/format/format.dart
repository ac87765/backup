/// 界面展示用的格式化函数（中文文案、十进制单位）。
///
/// 按 `design/desktop/` 界面稿：字节用 KB/MB/GB（1000 进制，Web 端用 KiB 等二进制单位，两者不同），
/// 时间显示为“今天 09:12 / 昨天 18:40 / 10月27日 16:22”。
library;

/// 文件的十进制大小，例如 `2.4 MB`、`186 KB`。
String formatBytes(int? bytes) {
  if (bytes == null) {
    return '—';
  }
  if (bytes < 0) {
    return '—';
  }
  if (bytes < 1000) {
    return '$bytes B';
  }
  const units = <String>['KB', 'MB', 'GB', 'TB', 'PB'];
  var value = bytes / 1000;
  var unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  // 与设计稿一致：KB / MB 小于 10 时保留一位小数（2.4 MB、48 KB、412 MB）；
  // GB 及以上小于 100 时保留一位小数（12.4 GB、37.6 GB），配额对比需要这个精度。
  final decimals = unit >= 2 ? value < 100 : value < 10;
  var text = decimals ? value.toStringAsFixed(1) : value.round().toString();
  // 整数值不显示 “.0”（设计稿：50 GB，而不是 50.0 GB）。
  if (text.endsWith('.0')) {
    text = text.substring(0, text.length - 2);
  }
  return '$text ${units[unit]}';
}

/// 传输速度，例如 `8.4 MB/s`。
String formatSpeed(double? bytesPerSecond) {
  if (bytesPerSecond == null || bytesPerSecond <= 0) {
    return '';
  }
  return '${formatBytes(bytesPerSecond.round())}/s';
}

/// 剩余时间，例如 `约 19 秒`、`约 2 分钟`。
String formatEta(Duration? remaining) {
  if (remaining == null || remaining.inSeconds < 0) {
    return '';
  }
  if (remaining.inSeconds < 60) {
    return '约 ${remaining.inSeconds} 秒';
  }
  if (remaining.inMinutes < 60) {
    return '约 ${remaining.inMinutes} 分钟';
  }
  return '约 ${remaining.inHours} 小时';
}

/// Unix 秒 → 本地时间文案；空值显示为 `—`。
String formatUnixTime(int? seconds, {DateTime? now}) {
  if (seconds == null || seconds <= 0) {
    return '—';
  }
  return formatDateTime(
    DateTime.fromMillisecondsSinceEpoch(seconds * 1000),
    now: now,
  );
}

/// ISO 8601 → 本地时间文案；无法解析时显示为 `—`。
String formatIsoTime(String? value, {DateTime? now}) {
  if (value == null || value.isEmpty) {
    return '—';
  }
  final parsed = DateTime.tryParse(value);
  if (parsed == null) {
    return '—';
  }
  return formatDateTime(parsed.toLocal(), now: now);
}

/// 设计稿的时间写法。
String formatDateTime(DateTime value, {DateTime? now}) {
  final current = now ?? DateTime.now();
  final local = value.toLocal();
  final today = DateTime(current.year, current.month, current.day);
  final day = DateTime(local.year, local.month, local.day);
  final clock = '${_pad(local.hour)}:${_pad(local.minute)}';
  final difference = today.difference(day).inDays;
  if (difference == 0) {
    return '今天 $clock';
  }
  if (difference == 1) {
    return '昨天 $clock';
  }
  if (local.year == current.year) {
    return '${local.month}月${local.day}日 $clock';
  }
  return '${local.year}年${local.month}月${local.day}日 $clock';
}

/// 最近活动时间：`刚刚` / `5 分钟前` / `10:05` / 日期。
String formatRelativeTime(int unixSeconds, {DateTime? now}) {
  if (unixSeconds <= 0) {
    return '—';
  }
  final at = DateTime.fromMillisecondsSinceEpoch(unixSeconds * 1000).toLocal();
  final current = now ?? DateTime.now();
  final difference = current.difference(at);
  if (difference.inSeconds < 60) {
    return '刚刚';
  }
  if (difference.inMinutes < 60) {
    return '${difference.inMinutes} 分钟前';
  }
  final today = DateTime(current.year, current.month, current.day);
  final day = DateTime(at.year, at.month, at.day);
  if (today == day) {
    return '${_pad(at.hour)}:${_pad(at.minute)}';
  }
  return formatDateTime(at, now: current);
}

/// Unix 秒 → 当天的 `HH:mm`（最近活动里的“内容未变”时间）。
String formatClockTime(int unixSeconds) {
  if (unixSeconds <= 0) {
    return '—';
  }
  final at = DateTime.fromMillisecondsSinceEpoch(unixSeconds * 1000).toLocal();
  return '${_pad(at.hour)}:${_pad(at.minute)}';
}

/// 备份文件夹卡片上的最近备份时间：一小时以内用相对时间，否则用日期时间。
String formatBackupTime(int? unixSeconds, {DateTime? now}) {
  if (unixSeconds == null || unixSeconds <= 0) {
    return '—';
  }
  final at = DateTime.fromMillisecondsSinceEpoch(unixSeconds * 1000).toLocal();
  final current = now ?? DateTime.now();
  final difference = current.difference(at);
  if (difference.inSeconds < 60) {
    return '刚刚';
  }
  if (difference.inMinutes < 60) {
    return '${difference.inMinutes} 分钟前';
  }
  return formatDateTime(at, now: current);
}

/// 千分位计数，例如 `2,140`。
String formatCount(int value) {
  final text = value.abs().toString();
  final buffer = StringBuffer();
  for (var index = 0; index < text.length; index += 1) {
    if (index > 0 && (text.length - index) % 3 == 0) {
      buffer.write(',');
    }
    buffer.write(text[index]);
  }
  return '${value < 0 ? '-' : ''}$buffer';
}

/// 同时兼容 Windows 与 POSIX 分隔符的文件名。
String baseName(String path) {
  final normalized = path.replaceAll('\\', '/');
  final trimmed = normalized.endsWith('/')
      ? normalized.substring(0, normalized.length - 1)
      : normalized;
  final index = trimmed.lastIndexOf('/');
  return index < 0 ? trimmed : trimmed.substring(index + 1);
}

/// 父目录（保留原分隔符风格）；没有分隔符时返回 `.`。
String parentPath(String path) {
  final separator = path.contains('\\') ? '\\' : '/';
  final normalized = path.replaceAll(separator == '\\' ? '/' : '\\', separator);
  final index = normalized.lastIndexOf(separator);
  if (index < 0) {
    return '.';
  }
  if (index == 0) {
    return separator;
  }
  // Windows 盘根：C:\file 的父目录是 C:\ 而不是 C:。
  if (separator == '\\' && index == 2 && normalized[1] == ':') {
    return normalized.substring(0, 3);
  }
  return normalized.substring(0, index);
}

String _pad(int value) => value.toString().padLeft(2, '0');
