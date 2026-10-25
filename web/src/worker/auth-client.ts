/* eslint-disable @typescript-eslint/only-throw-error -- Comlink 需要普通对象才能保留 code/retryAfter */
/**
 * Worker 内的认证客户端（design D12a/D12b）。
 *
 * - 所有认证请求都由这里用同源 fetch 发出；主线程不接触 auth_key/rk_auth。
 * - prepare / submit / commit 由 KeyStore 的待提交对象配合：服务端成功后才 commit，
 *   失败、取消或 lock() 一律 discard。
 * - 跨 Comlink 只返回用户信息、恢复密钥展示串与稳定错误码。
 */
import type { components } from '../api/schema'
import type {
  AuthErrorPayload,
  AuthWorkerApi,
  CryptoBackend,
  CurrentUser,
  PreparedRecoveryRotationView,
  PreparedRegistrationView,
  SessionSummaryView,
} from './types'

type PreloginResponse = components['schemas']['PreloginResponse']
type LoginResponse = components['schemas']['LoginResponse']
type MeResponse = components['schemas']['MeResponse']
type RecoveryPreloginResponse = components['schemas']['RecoveryPreloginResponse']
type SessionsResponse = components['schemas']['SessionsResponse']

/** 注册与新密码统一使用的推荐参数。 */
export const RECOMMENDED_KDF_PARAMS_JSON =
  '{"algorithm":"argon2id","m_cost_kib":65536,"t_cost":3,"p_cost":1}'

interface Profile {
  user: CurrentUser
  kdfParamsJson: string
  kdfSalt: Uint8Array
  wrappedMk: Uint8Array
}

interface PendingRegistration {
  email: string
  material: PreparedRegistrationView
}

interface PendingRecoveryRotation {
  material: PreparedRecoveryRotationView
}

function authError(
  code: string,
  message: string,
  retryAfter?: number,
): AuthErrorPayload {
  return retryAfter
    ? { code, message, retryAfter }
    : { code, message }
}

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')
}

function hexToBytes(hex: string): Uint8Array {
  if (hex.length % 2 !== 0) {
    throw authError('invalid_response', '服务端返回了非法的十六进制数据')
  }
  const output = new Uint8Array(hex.length / 2)
  for (let index = 0; index < output.length; index += 1) {
    const value = Number.parseInt(hex.slice(index * 2, index * 2 + 2), 16)
    if (Number.isNaN(value)) {
      throw authError('invalid_response', '服务端返回了非法的十六进制数据')
    }
    output[index] = value
  }
  return output
}

function zeroize(...values: Uint8Array[]): void {
  for (const value of values) {
    value.fill(0)
  }
}

export class AuthClient implements AuthWorkerApi {
  private readonly backend: Promise<CryptoBackend>
  private profile: Profile | null = null
  private pendingRegistration: PendingRegistration | null = null
  private pendingRecoveryRotation: PendingRecoveryRotation | null = null

  constructor(backend: Promise<CryptoBackend>) {
    this.backend = backend
  }

  async register(
    email: string,
    password: string,
  ): Promise<{ recoveryDisplay: string }> {
    const backend = await this.backend
    backend.lock()
    this.clearProfile()
    this.clearPendingRegistration()
    this.clearPendingRecoveryRotation()

    const salt = crypto.getRandomValues(new Uint8Array(16))
    const material = backend.prepareRegistration(
      password,
      RECOMMENDED_KDF_PARAMS_JSON,
      salt,
    )
    this.pendingRegistration = { email, material }
    return { recoveryDisplay: material.display }
  }

  async confirmRegistration(): Promise<void> {
    const backend = await this.backend
    const pending = this.pendingRegistration
    if (!pending) {
      throw authError('pending_missing', '没有待确认的注册材料')
    }
    const { material, email } = pending
    try {
      const body = JSON.stringify({
        email,
        kdf_params: JSON.parse(material.kdfParamsJson) as unknown,
        kdf_salt: bytesToHex(material.kdfSalt),
        auth_key: bytesToHex(material.authKey),
        wrapped_mk: bytesToHex(material.wrappedMk),
        recovery_wrapped_mk: bytesToHex(material.recoveryWrappedMk),
        recovery_auth_key: bytesToHex(material.recoveryAuthKey),
      })
      zeroize(
        material.authKey,
        material.recoveryAuthKey,
        material.recoveryWrappedMk,
        material.kdfSalt,
      )
      await this.request('/api/v1/auth/register', {
        method: 'POST',
        body,
      })
      backend.commitRegistration()
      this.clearPendingRegistration()
    } catch (error) {
      backend.discardPending()
      this.clearPendingRegistration()
      throw error
    }
  }

  async discardRegistration(): Promise<void> {
    const backend = await this.backend
    backend.discardPending()
    this.clearPendingRegistration()
  }

  async validateRecoveryKey(display: string): Promise<void> {
    const backend = await this.backend
    backend.validateRecoveryKey(display)
  }

  async login(email: string, password: string): Promise<CurrentUser> {
    const backend = await this.backend
    backend.lock()
    this.clearProfile()

    const prelogin = await this.request<PreloginResponse>(
      '/api/v1/auth/prelogin',
      { method: 'POST', body: JSON.stringify({ email }) },
    )
    const kdfParamsJson = JSON.stringify(prelogin.kdf_params)
    const salt = hexToBytes(prelogin.kdf_salt)
    const authKey = backend.derivePasswordKeys(password, kdfParamsJson, salt)

    const body = JSON.stringify({ email, auth_key: bytesToHex(authKey) })
    zeroize(authKey)
    let response: LoginResponse
    try {
      response = await this.request<LoginResponse>('/api/v1/auth/login', {
        method: 'POST',
        body,
      })
    } catch (error) {
      backend.lock()
      throw error
    }

    const wrappedMk = hexToBytes(response.wrapped_mk)
    try {
      backend.unlockWithPassword(wrappedMk)
    } catch (error) {
      backend.lock()
      throw error
    }
    this.setProfile({
      user: {
        userId: response.user_id,
        email: response.email,
        emailVerified: response.email_verified,
        rootNodeId: response.root_node_id,
      },
      kdfParamsJson: JSON.stringify(response.kdf_params),
      kdfSalt: hexToBytes(response.kdf_salt),
      wrappedMk,
    })
    if (!this.profile) {
      throw authError('internal_error', '登录状态未建立')
    }
    return this.profile.user
  }

  async refreshSession(): Promise<CurrentUser | null> {
    try {
      const response = await this.request<MeResponse>('/api/v1/auth/me', {
        method: 'GET',
      })
      this.setProfile({
        user: {
          userId: response.user_id,
          email: response.email,
          emailVerified: response.email_verified,
          rootNodeId: response.root_node_id,
        },
        kdfParamsJson: JSON.stringify(response.kdf_params),
        kdfSalt: hexToBytes(response.kdf_salt),
        wrappedMk: hexToBytes(response.wrapped_mk),
      })
      return this.profile?.user ?? null
    } catch (error) {
      if ((error as AuthErrorPayload).code === 'unauthenticated') {
        const backend = await this.backend
        backend.lock()
        this.clearProfile()
        return null
      }
      throw error
    }
  }

  async unlock(password: string): Promise<void> {
    const backend = await this.backend
    const profile = await this.requireProfile()
    backend.lock()
    const authKey = backend.derivePasswordKeys(
      password,
      profile.kdfParamsJson,
      profile.kdfSalt,
    )
    try {
      backend.unlockWithPassword(profile.wrappedMk)
    } catch (error) {
      backend.lock()
      throw error
    } finally {
      zeroize(authKey)
    }
  }

  async changePassword(oldPassword: string, newPassword: string): Promise<void> {
    const backend = await this.backend
    const profile = await this.requireProfile()
    if (backend.isLocked()) {
      throw authError('locked', '密钥库已锁定，请先解锁')
    }
    const prepared = backend.preparePasswordChange(
      oldPassword,
      profile.kdfParamsJson,
      profile.kdfSalt,
      newPassword,
    )
    const newKdfSalt = prepared.newKdfSalt.slice()
    const newWrappedMk = prepared.newWrappedMk.slice()
    const body = JSON.stringify({
      old_auth_key: bytesToHex(prepared.oldAuthKey),
      new_auth_key: bytesToHex(prepared.newAuthKey),
      new_kdf_params: JSON.parse(prepared.newKdfParamsJson) as unknown,
      new_kdf_salt: bytesToHex(prepared.newKdfSalt),
      new_wrapped_mk: bytesToHex(prepared.newWrappedMk),
    })
    zeroize(prepared.oldAuthKey, prepared.newAuthKey)
    try {
      await this.request('/api/v1/auth/change-password', {
        method: 'POST',
        body,
      })
      backend.commitPasswordChange()
      // 后续操作使用新参数/盐与新的 wrapped_mk（MK 本身未变）。
      this.setProfile({
        ...profile,
        kdfParamsJson: prepared.newKdfParamsJson,
        kdfSalt: newKdfSalt,
        wrappedMk: newWrappedMk,
      })
    } catch (error) {
      backend.discardPending()
      zeroize(newKdfSalt, newWrappedMk)
      throw error
    } finally {
      zeroize(prepared.newKdfSalt, prepared.newWrappedMk)
    }
  }

  async resetWithRecovery(
    email: string,
    recoveryKey: string,
    newPassword: string,
  ): Promise<void> {
    const backend = await this.backend
    // 恢复密钥的校验位/字符集先本地校验；抄错时不发送任何 HTTP 请求。
    backend.validateRecoveryKey(recoveryKey)

    const recovery = await this.request<RecoveryPreloginResponse>(
      '/api/v1/auth/recovery-prelogin',
      { method: 'POST', body: JSON.stringify({ email }) },
    )
    const recoveryWrappedMk = hexToBytes(recovery.recovery_wrapped_mk)
    const prepared = backend.prepareRecoveryReset(
      recoveryKey,
      recoveryWrappedMk,
      newPassword,
    )
    const body = JSON.stringify({
      email,
      recovery_auth_key: bytesToHex(prepared.recoveryAuthKey),
      new_auth_key: bytesToHex(prepared.newAuthKey),
      new_kdf_params: JSON.parse(prepared.newKdfParamsJson) as unknown,
      new_kdf_salt: bytesToHex(prepared.newKdfSalt),
      new_wrapped_mk: bytesToHex(prepared.newWrappedMk),
    })
    zeroize(
      recoveryWrappedMk,
      prepared.recoveryAuthKey,
      prepared.newAuthKey,
      prepared.newKdfSalt,
      prepared.newWrappedMk,
    )
    try {
      await this.request('/api/v1/auth/recover', { method: 'POST', body })
      backend.commitRecoveryReset()
      this.clearProfile()
    } catch (error) {
      backend.discardPending()
      throw error
    }
  }

  async prepareRecoveryRotation(
    currentPassword: string,
  ): Promise<{ recoveryDisplay: string }> {
    const backend = await this.backend
    const profile = await this.requireProfile()
    if (backend.isLocked()) {
      throw authError('locked', '密钥库已锁定，请先解锁')
    }
    this.clearPendingRecoveryRotation()
    const material = backend.prepareRecoveryRotation(
      currentPassword,
      profile.kdfParamsJson,
      profile.kdfSalt,
    )
    this.pendingRecoveryRotation = { material }
    return { recoveryDisplay: material.display }
  }

  async confirmRecoveryRotation(): Promise<void> {
    const backend = await this.backend
    const pending = this.pendingRecoveryRotation
    if (!pending) {
      throw authError('pending_missing', '没有待确认的恢复密钥材料')
    }
    const { material } = pending
    const body = JSON.stringify({
      current_auth_key: bytesToHex(material.currentAuthKey),
      new_recovery_wrapped_mk: bytesToHex(material.recoveryWrappedMk),
      new_recovery_auth_key: bytesToHex(material.recoveryAuthKey),
    })
    zeroize(
      material.currentAuthKey,
      material.recoveryWrappedMk,
      material.recoveryAuthKey,
    )
    try {
      await this.request('/api/v1/auth/rotate-recovery-key', {
        method: 'POST',
        body,
      })
      backend.commitRecoveryRotation()
      this.clearPendingRecoveryRotation()
    } catch (error) {
      backend.discardPending()
      this.clearPendingRecoveryRotation()
      throw error
    }
  }

  async discardRecoveryRotation(): Promise<void> {
    const backend = await this.backend
    backend.discardPending()
    this.clearPendingRecoveryRotation()
  }

  async logout(): Promise<void> {
    try {
      await this.request('/api/v1/auth/logout', { method: 'POST' })
    } finally {
      const backend = await this.backend
      backend.lock()
      this.clearProfile()
      this.clearPendingRegistration()
      this.clearPendingRecoveryRotation()
    }
  }

  async verifyEmail(token: string): Promise<void> {
    await this.request('/api/v1/auth/verify-email', {
      method: 'POST',
      body: JSON.stringify({ token }),
    })
  }

  async resendVerification(email: string): Promise<void> {
    await this.request('/api/v1/auth/resend-verification', {
      method: 'POST',
      body: JSON.stringify({ email }),
    })
  }

  async getSessions(): Promise<SessionSummaryView[]> {
    const response = await this.request<SessionsResponse>(
      '/api/v1/auth/sessions',
      { method: 'GET' },
    )
    return response.sessions.map((session) => ({
      id: session.id,
      createdAt: session.created_at,
      lastSeenAt: session.last_seen_at,
      expiresAt: session.expires_at,
      ip: session.ip,
      userAgent: session.user_agent ?? null,
      current: session.current,
    }))
  }

  async revokeSession(sessionId: string): Promise<void> {
    await this.request(
      `/api/v1/auth/sessions/${encodeURIComponent(sessionId)}`,
      { method: 'DELETE' },
    )
  }

  private async requireProfile(): Promise<Profile> {
    if (this.profile) {
      return this.profile
    }
    const user = await this.refreshSession()
    if (!user || !this.profile) {
      throw authError('unauthenticated', '需要登录')
    }
    return this.profile
  }

  private setProfile(profile: Profile): void {
    this.clearProfile()
    this.profile = profile
  }

  private clearProfile(): void {
    if (this.profile) {
      zeroize(this.profile.kdfSalt, this.profile.wrappedMk)
      this.profile = null
    }
  }

  private clearPendingRegistration(): void {
    if (this.pendingRegistration) {
      const { material } = this.pendingRegistration
      zeroize(
        material.authKey,
        material.recoveryAuthKey,
        material.recoveryWrappedMk,
        material.kdfSalt,
      )
      this.pendingRegistration = null
    }
  }

  private clearPendingRecoveryRotation(): void {
    if (this.pendingRecoveryRotation) {
      const { material } = this.pendingRecoveryRotation
      zeroize(
        material.currentAuthKey,
        material.recoveryAuthKey,
        material.recoveryWrappedMk,
      )
      this.pendingRecoveryRotation = null
    }
  }

  private async request<T = unknown>(
    path: string,
    init: { method: string; body?: string },
  ): Promise<T> {
    let response: Response
    try {
      response = await fetch(path, {
        method: init.method,
        credentials: 'same-origin',
        headers: {
          'X-Requested-With': 'backup-web',
          ...(init.body ? { 'Content-Type': 'application/json' } : {}),
        },
        body: init.body,
      })
    } catch {
      throw authError('network_error', '网络请求失败，请稍后重试')
    }

    const text = await response.text()
    let payload: unknown = undefined
    if (text) {
      try {
        payload = JSON.parse(text) as unknown
      } catch {
        payload = undefined
      }
    }
    if (!response.ok) {
      const error = payload as { code?: unknown; message?: unknown } | undefined
      const retryAfter = Number(response.headers.get('retry-after'))
      throw authError(
        typeof error?.code === 'string' ? error.code : 'http_error',
        typeof error?.message === 'string' ? error.message : '请求失败',
        Number.isFinite(retryAfter) && retryAfter > 0 ? retryAfter : undefined,
      )
    }
    return payload as T
  }
}

/** 创建认证 API（Worker 内使用）。 */
export function createAuthApi(backend: Promise<CryptoBackend>): AuthWorkerApi {
  return new AuthClient(backend)
}
