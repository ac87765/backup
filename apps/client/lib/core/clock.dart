import 'package:flutter_riverpod/flutter_riverpod.dart';

/// 当前时间来源。传输速度/剩余时间依赖它；测试可注入可控时钟。
typedef Clock = DateTime Function();

final clockProvider = Provider<Clock>((ref) => DateTime.now);
