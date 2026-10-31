/// 桥接层公开类型（由 flutter_rust_bridge 生成的 DTO）。
///
/// 界面只从这里取类型，不直接依赖 `lib/src/rust` 的内部路径，便于之后
/// iOS 复用 core 层。
library;

export '../../src/rust/api/error.dart' show BridgeError;
export '../../src/rust/api/types.dart';
