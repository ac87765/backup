import 'dart:async';
import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:tray_manager/tray_manager.dart';

import '../../app.dart';
import '../../common/state/auth_controller.dart';
import '../../core/platform/external_links.dart';
import '../../core/platform/window_controls.dart';
import '../../core/providers/app_providers.dart';
import '../../core/rust/models.dart';
import 'tray_model.dart';

/// 系统托盘 / 菜单栏（design D7 第一版）：
///
/// - 图标四种状态（正常 / 正在上传 / 暂停 / 需要解锁）；
/// - 原生菜单：状态文字（禁用）、暂停或恢复、打开应用、打开网页版、设置、退出；
/// - 左键单击显示并聚焦主窗口；需要解锁时打开解锁页；
/// - 状态来自备份引擎的 summary 事件（直接订阅桥接事件流，避免丢事件）。
class TrayService {
  TrayService(this._ref);

  final Ref _ref;

  TrayIcon? _tray;
  Menu? _menu;
  ListenerId? _trayListener;
  ListenerId? _menuListener;
  StreamSubscription<BackupEventDto>? _events;
  final Map<MenuItemId, Future<void> Function()> _actions =
      <MenuItemId, Future<void> Function()>{};
  // 菜单项的原生句柄归 Dart 对象所有；必须持有引用，否则会被 GC 释放。
  final List<MenuItem> _items = <MenuItem>[];
  BackupSummaryDto? _summary;
  AuthStage _stage = AuthStage.initializing;
  bool _started = false;
  bool _available = false;
  bool _rendering = false;
  bool _engineStarted = false;

  /// 返回托盘是否可用；不可用时调用方不应把关闭按钮变成“隐藏到托盘”。
  Future<bool> start() async {
    if (_started) {
      return _available;
    }
    _started = true;
    if (!Platform.isWindows && !Platform.isMacOS) {
      return false;
    }
    final tray = await _createTray();
    if (tray == null) {
      return false;
    }
    final menu = Menu.create();
    if (menu == null) {
      tray.dispose();
      return false;
    }
    _tray = tray;
    _menu = menu;
    _available = true;
    tray.setTooltip('加密备份');
    tray.setVisible(true);
    if (Platform.isMacOS) {
      // @1x 模板图 22px；系统按菜单栏深浅色自动着色。
      tray.isIconTemplate = true;
    }
    _trayListener = tray.addListener(_onTrayEvent);
    _menuListener = menu.addListener(_onMenuEvent);
    tray.setContextMenu(menu);
    // 左键点击图标打开主窗口，菜单放在右键。
    tray.setContextMenuTrigger(ContextMenuTrigger.rightClicked);

    _events = _ref.read(nativeBridgeProvider).backupEventsStream().listen((
      event,
    ) {
      final summary = event.summary;
      if (summary != null) {
        _summary = summary;
        _render();
      }
    });
    try {
      _summary = await _ref.read(nativeBridgeProvider).backupSummary();
    } catch (_) {
      // 登录前或引擎未启动时没有汇总，托盘显示“备份状态不可用”。
    }
    _render();
    return true;
  }

  Future<TrayIcon?> _createTray() async {
    try {
      if (!TrayManager.instance.isSupported()) {
        return null;
      }
      return TrayIcon.create();
    } catch (_) {
      // FFI / 平台不支持时退回“关闭即退出”，不把窗口藏到不存在的托盘里。
      return null;
    }
  }

  /// 认证阶段变化（由 provider 的 listen 调用）。
  void updateAuth(AuthStage stage) {
    _stage = stage;
    _render();
    if (stage == AuthStage.ready) {
      unawaited(_ensureEngine());
    } else if (stage == AuthStage.login && _engineStarted) {
      _engineStarted = false;
      unawaited(_stopEngine());
    }
  }

  Future<void> _stopEngine() async {
    try {
      await _ref.read(nativeBridgeProvider).backupStop();
    } catch (_) {
      // 退出登录时尽力停止；失败不影响登录页。
    }
  }

  /// 登录 / 解锁后启动备份引擎（幂等）；关闭窗口后备份继续依赖它。
  Future<void> _ensureEngine() async {
    if (_engineStarted) {
      return;
    }
    _engineStarted = true;
    try {
      await _ref.read(nativeBridgeProvider).backupStart();
    } catch (_) {
      // 引擎启动失败时保留状态栏/托盘的错误提示；下次认证变化再试。
      _engineStarted = false;
    }
  }

  void _onTrayEvent(TrayIconEvent event) {
    if (event is TrayIconClickedEvent || event is TrayIconDoubleClickedEvent) {
      unawaited(_openApp());
    }
  }

  void _onMenuEvent(MenuEvent event) {
    if (event is! MenuItemClickedEvent) {
      return;
    }
    final action = _actions[event.itemId];
    if (action != null) {
      unawaited(action());
    }
  }

  void _render() {
    if (_rendering) {
      return;
    }
    _rendering = true;
    try {
      final model = trayMenuModelFor(
        _summary,
        needsUnlock: _stage == AuthStage.unlock,
      );
      final tray = _tray;
      if (tray != null) {
        final image = ImageAsset.fromAsset(
          trayIconAsset(model.icon, macOS: Platform.isMacOS),
        );
        if (image != null) {
          tray.icon = image;
        }
        tray.setTooltip(model.tooltip);
      }
      _renderMenu(model);
    } finally {
      _rendering = false;
    }
  }

  void _renderMenu(TrayMenuModel model) {
    final menu = _menu;
    if (menu == null) {
      return;
    }
    menu.clear();
    _actions.clear();
    _items.clear();

    _addItem(menu, model.statusLabel, enabled: false);
    menu.addSeparator();
    _addItem(
      menu,
      model.pauseResumeLabel,
      enabled: model.pauseResumeEnabled,
      action: model.pauseResumeLabel == '恢复备份' ? _resumeBackup : _pauseBackup,
    );
    _addItem(menu, '打开应用', action: _openApp);
    _addItem(menu, '打开网页版', action: _openWeb);
    _addItem(menu, '设置', action: _openSettings);
    menu.addSeparator();
    _addItem(menu, '退出', action: _quit);
  }

  void _addItem(
    Menu menu,
    String label, {
    bool enabled = true,
    Future<void> Function()? action,
  }) {
    final item = MenuItem.createWithLabelAndType(label, MenuItemType.normal);
    if (item == null) {
      return;
    }
    item.isEnabled = enabled;
    if (action != null) {
      _actions[item.id] = action;
    }
    _items.add(item);
    menu.addItem(item);
  }

  Future<void> _openApp() async {
    await _ref.read(windowControlsProvider).showAndFocus();
    if (_stage == AuthStage.unlock) {
      _ref.read(routerProvider).go('/unlock');
    }
  }

  Future<void> _openWeb() async {
    try {
      final config = await _ref.read(appConfigProvider.future);
      await _ref.read(externalLinkOpenerProvider)(
        webRoute(config.serverUrl, '/'),
      );
    } catch (_) {
      // 地址未配置或系统浏览器不可用时不打断托盘操作。
    }
  }

  Future<void> _openSettings() async {
    await _ref.read(windowControlsProvider).showAndFocus();
    _ref.read(routerProvider).go('/settings');
  }

  Future<void> _pauseBackup() async {
    try {
      await _ref.read(nativeBridgeProvider).backupPauseAll();
    } catch (_) {
      // 引擎不可用时菜单文字由下一次 summary 事件纠正。
    }
  }

  Future<void> _resumeBackup() async {
    try {
      await _ref.read(nativeBridgeProvider).backupResumeAll();
    } catch (_) {
      // 同上。
    }
  }

  Future<void> _quit() async {
    try {
      await _ref.read(nativeBridgeProvider).backupStop();
    } catch (_) {
      // 退出前尽量停止引擎；失败也要允许退出。
    }
    await _ref.read(windowControlsProvider).quit();
  }

  void dispose() {
    unawaited(_events?.cancel());
    final tray = _tray;
    final menu = _menu;
    if (tray != null) {
      final listener = _trayListener;
      if (listener != null) {
        tray.removeListener(listener);
      }
      tray.setVisible(false);
      tray.dispose();
    }
    if (menu != null) {
      final listener = _menuListener;
      if (listener != null) {
        menu.removeListener(listener);
      }
      menu.dispose();
    }
  }
}

final trayServiceProvider = Provider<TrayService>((ref) {
  final service = TrayService(ref);
  service.updateAuth(ref.read(authControllerProvider).stage);
  ref.listen(authControllerProvider, (previous, next) {
    service.updateAuth(next.stage);
  });
  ref.onDispose(service.dispose);
  return service;
});
