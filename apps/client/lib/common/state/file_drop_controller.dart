import 'package:flutter_riverpod/flutter_riverpod.dart';

/// 文件页的拖拽放置提示是否可见。
///
/// 抽成 provider 让组件测试 / golden 可以直接展示 drop 状态，不依赖平台
/// 拖拽通道。
class FileDropOverlayController extends Notifier<bool> {
  @override
  bool build() => false;

  void setVisible(bool visible) {
    if (state != visible) {
      state = visible;
    }
  }
}

/// autoDispose：文件页卸载（唯一监听者离开）后自动复位为不可见，
/// 不需要在 State.dispose 中使用 ref。
final fileDropOverlayProvider =
    NotifierProvider.autoDispose<FileDropOverlayController, bool>(
      FileDropOverlayController.new,
    );
