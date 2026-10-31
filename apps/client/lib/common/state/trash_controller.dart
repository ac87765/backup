import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/errors/bridge_error_text.dart';
import '../../core/providers/app_providers.dart';
import '../../core/rust/models.dart';

@immutable
class TrashState {
  const TrashState({
    this.items = const <TrashItemDto>[],
    this.loading = true,
    this.error,
    this.busy = false,
  });

  final List<TrashItemDto> items;
  final bool loading;
  final String? error;
  final bool busy;

  TrashState copyWith({
    List<TrashItemDto>? items,
    bool? loading,
    String? error,
    bool clearError = false,
    bool? busy,
  }) {
    return TrashState(
      items: items ?? this.items,
      loading: loading ?? this.loading,
      error: clearError ? null : (error ?? this.error),
      busy: busy ?? this.busy,
    );
  }
}

/// 回收站列表、恢复与永久删除。
class TrashController extends Notifier<TrashState> {
  @override
  TrashState build() {
    Future<void>.microtask(refresh);
    return const TrashState();
  }

  Future<void> refresh() async {
    state = state.copyWith(loading: true, clearError: true);
    try {
      final items = await ref.read(nativeBridgeProvider).listTrash();
      if (!ref.mounted) {
        return;
      }
      final sorted = [...items]
        ..sort((left, right) => left.node.name.compareTo(right.node.name));
      state = state.copyWith(items: sorted, loading: false, clearError: true);
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(loading: false, error: bridgeErrorText(error));
    }
  }

  Future<void> restore(TrashItemDto item) async {
    state = state.copyWith(busy: true, clearError: true);
    try {
      await ref.read(nativeBridgeProvider).restoreFromTrash(item.node.id, null);
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(
        items: state.items
            .where((entry) => entry.node.id != item.node.id)
            .toList(growable: false),
        busy: false,
      );
      ref.invalidate(usageProvider);
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(busy: false, error: bridgeErrorText(error));
    }
  }

  Future<PermanentDeleteOutcomeDto?> permanentlyDelete(
    TrashItemDto item,
  ) async {
    state = state.copyWith(busy: true, clearError: true);
    try {
      final outcome = await ref
          .read(nativeBridgeProvider)
          .permanentlyDelete(item.node.id);
      if (!ref.mounted) {
        return null;
      }
      state = state.copyWith(
        items: state.items
            .where((entry) => entry.node.id != item.node.id)
            .toList(growable: false),
        busy: false,
      );
      ref.invalidate(usageProvider);
      return outcome;
    } catch (error) {
      if (!ref.mounted) {
        return null;
      }
      state = state.copyWith(busy: false, error: bridgeErrorText(error));
      return null;
    }
  }
}

final trashControllerProvider = NotifierProvider<TrashController, TrashState>(
  TrashController.new,
);
