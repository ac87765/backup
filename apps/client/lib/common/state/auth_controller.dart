import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../core/errors/bridge_error_text.dart';
import '../../core/providers/app_providers.dart';
import '../../core/rust/models.dart';

/// 认证阶段，路由守卫按它决定去哪个页面。
enum AuthStage {
  /// 正在初始化 Rust 核心 / 恢复会话。
  initializing,

  /// 没有可用会话，需要邮箱 + 密码登录。
  login,

  /// 有会话但内存中没有主密钥，需要输入密码解锁。
  unlock,

  /// 账号待审核：显示等待说明，不提供任何写入口。
  pending,

  /// 已登录且已解锁，可进入文件页。
  ready,

  /// 启动失败（例如本地状态库无法打开），显示重试入口。
  failure,
}

@immutable
class AuthState {
  const AuthState({
    required this.stage,
    this.account,
    this.busy = false,
    this.error,
    this.keepUnlocked = false,
  });

  final AuthStage stage;
  final AccountDto? account;
  final bool busy;
  final String? error;

  /// “在这台电脑上保持解锁”当前是否开启（默认关闭）。
  final bool keepUnlocked;

  AuthState copyWith({
    AuthStage? stage,
    AccountDto? account,
    bool? busy,
    String? error,
    bool clearError = false,
    bool? keepUnlocked,
  }) {
    return AuthState(
      stage: stage ?? this.stage,
      account: account ?? this.account,
      busy: busy ?? this.busy,
      error: clearError ? null : (error ?? this.error),
      keepUnlocked: keepUnlocked ?? this.keepUnlocked,
    );
  }
}

/// 登录 / 解锁 / 待审核 / 保持解锁的唯一状态源。
///
/// 密码只作为方法参数传给桥接，不进入 [AuthState]，也不写日志或本地存储。
class AuthController extends Notifier<AuthState> {
  @override
  AuthState build() {
    // 启动流程是异步的：页面先显示启动页，结果通过 state 更新驱动路由守卫。
    Future<void>.microtask(_bootstrap);
    return const AuthState(stage: AuthStage.initializing);
  }

  Future<void> _bootstrap() async {
    try {
      await ref.read(bootstrapProvider.future);
      final bridge = ref.read(nativeBridgeProvider);
      final keepUnlocked = await bridge.isKeepUnlocked();
      if (!ref.mounted) {
        return;
      }
      final hasSession = await bridge.hasStoredSession();
      if (!hasSession) {
        state = AuthState(stage: AuthStage.login, keepUnlocked: keepUnlocked);
        return;
      }
      final account = await bridge.restoreSession();
      if (!ref.mounted) {
        return;
      }
      if (account == null) {
        state = AuthState(stage: AuthStage.login, keepUnlocked: keepUnlocked);
        return;
      }
      final restored = await _resolveAccount(account);
      if (!ref.mounted) {
        return;
      }
      state = restored.copyWith(keepUnlocked: keepUnlocked);
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      // 恢复会话失败时优先回到可操作页面：有会话 → 解锁，否则登录。
      var hasSession = false;
      try {
        hasSession = await ref.read(nativeBridgeProvider).hasStoredSession();
      } catch (_) {
        // 本地钥匙串也不可用时只能回登录页。
      }
      if (!ref.mounted) {
        return;
      }
      state = AuthState(
        stage: hasSession ? AuthStage.unlock : AuthStage.login,
        error: bridgeErrorText(error),
      );
    }
  }

  /// 有会话但未解锁时尝试“保持解锁”；没有保存的主密钥则去解锁页。
  Future<AuthState> _resolveAccount(AccountDto account) async {
    if (_isPending(account)) {
      return AuthState(stage: AuthStage.pending, account: account);
    }
    if (account.unlocked) {
      return AuthState(stage: AuthStage.ready, account: account);
    }
    final stored = await ref
        .read(nativeBridgeProvider)
        .unlockWithStoredMasterKey();
    if (stored != null) {
      if (_isPending(stored)) {
        return AuthState(stage: AuthStage.pending, account: stored);
      }
      return AuthState(stage: AuthStage.ready, account: stored);
    }
    return AuthState(stage: AuthStage.unlock, account: account);
  }

  /// 邮箱 + 密码登录。待审核账号进入 pending 页。
  Future<void> login(String email, String password) async {
    state = state.copyWith(busy: true, clearError: true);
    try {
      final account = await ref
          .read(nativeBridgeProvider)
          .login(email.trim(), password);
      if (!ref.mounted) {
        return;
      }
      if (_isPending(account)) {
        state = AuthState(
          stage: AuthStage.pending,
          account: account,
          keepUnlocked: state.keepUnlocked,
        );
      } else {
        state = AuthState(
          stage: AuthStage.ready,
          account: account,
          keepUnlocked: state.keepUnlocked,
        );
      }
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(
        stage: AuthStage.login,
        busy: false,
        error: bridgeErrorText(error),
      );
    }
  }

  /// 只输入密码解锁已保存的会话。
  Future<void> unlock(String password) async {
    state = state.copyWith(busy: true, clearError: true);
    try {
      final account = await ref.read(nativeBridgeProvider).unlock(password);
      if (!ref.mounted) {
        return;
      }
      state = AuthState(
        stage: _isPending(account) ? AuthStage.pending : AuthStage.ready,
        account: account,
        keepUnlocked: state.keepUnlocked,
      );
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(
        stage: AuthStage.unlock,
        busy: false,
        error: bridgeErrorText(error),
      );
    }
  }

  /// 待审核页“重新检查”：账号通过后直接进入文件页。
  Future<void> refreshAccount() async {
    if (state.stage != AuthStage.pending) {
      return;
    }
    state = state.copyWith(busy: true, clearError: true);
    try {
      final account = await ref.read(nativeBridgeProvider).currentAccount();
      if (!ref.mounted) {
        return;
      }
      if (_isPending(account)) {
        state = _pendingState(account);
      } else {
        state = AuthState(
          stage: AuthStage.ready,
          account: account,
          keepUnlocked: state.keepUnlocked,
        );
        // 审核通过后补启动备份引擎（登录时 pending 会失败）。
        try {
          await ref.read(nativeBridgeProvider).backupStart();
        } catch (_) {
          // 备份引擎启动失败不影响浏览；状态栏会显示需要解锁/错误。
        }
      }
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(busy: false, error: bridgeErrorText(error));
    }
  }

  /// 解锁页“切换账号”：清除会话与主密钥后回到登录页。
  Future<void> switchAccount() async {
    state = state.copyWith(busy: true, clearError: true);
    try {
      await ref.read(nativeBridgeProvider).forgetCredentials();
    } catch (_) {
      // 即使网络不可用也要回到登录页；本地凭据由 Rust 尽力清除。
    }
    if (!ref.mounted) {
      return;
    }
    state = const AuthState(stage: AuthStage.login);
  }

  /// 退出登录：服务端撤销会话并清除本机会话与主密钥。
  Future<void> logout() async {
    state = state.copyWith(busy: true, clearError: true);
    try {
      await ref.read(nativeBridgeProvider).logout();
    } catch (_) {
      // 忽略网络错误：仍回到登录页，下次启动会重新校验会话。
    }
    if (!ref.mounted) {
      return;
    }
    state = const AuthState(stage: AuthStage.login);
  }

  /// 开启“保持解锁”：先用密码验证当前账号，再把主密钥交给系统钥匙串。
  ///
  /// 密码只传给桥接；方法返回后不保留任何引用。
  Future<void> enableKeepUnlocked(String password) async {
    final bridge = ref.read(nativeBridgeProvider);
    await bridge.unlock(password);
    await bridge.setKeepUnlocked(true);
    if (!ref.mounted) {
      return;
    }
    state = state.copyWith(keepUnlocked: true, busy: false, clearError: true);
  }

  /// 关闭“保持解锁”：删除钥匙串中的主密钥。
  Future<void> disableKeepUnlocked() async {
    state = state.copyWith(busy: true, clearError: true);
    try {
      await ref.read(nativeBridgeProvider).setKeepUnlocked(false);
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(keepUnlocked: false, busy: false);
    } catch (error) {
      if (!ref.mounted) {
        return;
      }
      state = state.copyWith(busy: false, error: bridgeErrorText(error));
    }
  }

  /// 清除启动/登录页的错误（用户重新输入时）。
  void clearError() {
    if (state.error != null) {
      state = state.copyWith(clearError: true);
    }
  }

  bool _isPending(AccountDto account) =>
      account.status == AccountStatusDto.pending;

  AuthState _pendingState(AccountDto account) => AuthState(
    stage: AuthStage.pending,
    account: account,
    keepUnlocked: state.keepUnlocked,
  );
}

final authControllerProvider = NotifierProvider<AuthController, AuthState>(
  AuthController.new,
);
