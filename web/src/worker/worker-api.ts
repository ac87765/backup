/**
 * Worker 暴露给主线程的完整 API：加密方法 + 认证高层方法。
 *
 * 认证方法内部复用同一个 KeyStore（`CryptoApi` 与 `AuthClient` 共享同一个后端 Promise），
 * 因此 prepare/commit 的状态变化对两侧一致。
 */
import { AuthClient } from './auth-client'
import { CryptoApi } from './crypto-api'
import type {
  AuthErrorPayload,
  AuthWorkerApi,
  CryptoBackend,
  CurrentUser,
  SessionSummaryView,
} from './types'

/** 把 WASM/网络错误统一成可跨 Comlink 的普通对象（保留 code/retryAfter）。 */
function toAuthPayload(error: unknown): AuthErrorPayload {
  const candidate = error as
    | { code?: unknown; message?: unknown; retryAfter?: unknown }
    | null
  return {
    code:
      candidate && typeof candidate.code === 'string'
        ? candidate.code
        : 'crypto_error',
    message:
      candidate && typeof candidate.message === 'string'
        ? candidate.message
        : '认证操作失败',
    ...(candidate && typeof candidate.retryAfter === 'number'
      ? { retryAfter: candidate.retryAfter }
      : {}),
  }
}

async function runAuth<T>(operation: () => Promise<T>): Promise<T> {
  try {
    return await operation()
  } catch (error) {
    // Comlink 只可靠传输结构化克隆数据；Error 上的自定义 code 会丢失。
    // eslint-disable-next-line @typescript-eslint/only-throw-error
    throw toAuthPayload(error)
  }
}

export class WorkerApi extends CryptoApi implements AuthWorkerApi {
  private readonly auth: AuthClient

  constructor(backend: CryptoBackend | Promise<CryptoBackend>) {
    const promise = Promise.resolve(backend)
    super(promise)
    this.auth = new AuthClient(promise)
  }

  register(
    email: string,
    password: string,
  ): Promise<{ recoveryDisplay: string }> {
    return runAuth(() => this.auth.register(email, password))
  }

  confirmRegistration(): Promise<void> {
    return runAuth(() => this.auth.confirmRegistration())
  }

  discardRegistration(): Promise<void> {
    return runAuth(() => this.auth.discardRegistration())
  }

  validateRecoveryKey(display: string): Promise<void> {
    return runAuth(() => this.auth.validateRecoveryKey(display))
  }

  login(email: string, password: string): Promise<CurrentUser> {
    return runAuth(() => this.auth.login(email, password))
  }

  refreshSession(): Promise<CurrentUser | null> {
    return runAuth(() => this.auth.refreshSession())
  }

  unlock(password: string): Promise<void> {
    return runAuth(() => this.auth.unlock(password))
  }

  changePassword(oldPassword: string, newPassword: string): Promise<void> {
    return runAuth(() => this.auth.changePassword(oldPassword, newPassword))
  }

  resetWithRecovery(
    email: string,
    recoveryKey: string,
    newPassword: string,
  ): Promise<void> {
    return runAuth(() =>
      this.auth.resetWithRecovery(email, recoveryKey, newPassword),
    )
  }

  prepareRecoveryRotation(
    currentPassword: string,
  ): Promise<{ recoveryDisplay: string }> {
    return runAuth(() => this.auth.prepareRecoveryRotation(currentPassword))
  }

  confirmRecoveryRotation(): Promise<void> {
    return runAuth(() => this.auth.confirmRecoveryRotation())
  }

  discardRecoveryRotation(): Promise<void> {
    return runAuth(() => this.auth.discardRecoveryRotation())
  }

  logout(): Promise<void> {
    return runAuth(() => this.auth.logout())
  }

  verifyEmail(token: string): Promise<void> {
    return runAuth(() => this.auth.verifyEmail(token))
  }

  resendVerification(email: string): Promise<void> {
    return runAuth(() => this.auth.resendVerification(email))
  }

  getSessions(): Promise<SessionSummaryView[]> {
    return runAuth(() => this.auth.getSessions())
  }

  revokeSession(sessionId: string): Promise<void> {
    return runAuth(() => this.auth.revokeSession(sessionId))
  }
}
