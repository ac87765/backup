/**
 * Worker 与主线程共享的类型定义。
 *
 * **本文件不得引入 WASM**：它同时被主线程代码 import。
 * Comlink 暴露的 API 中，入参与返回值都不允许出现密钥字节：
 * - 返回 `Uint8Array` 的方法只能是密文（包裹后的密钥、元数据密文、块密文）或明文（块明文）；
 * - 认证密钥、恢复认证值等要发给服务端的派生值由 Worker 自己完成 HTTP 请求（第 6 组），
 *   不经过 Comlink。
 */

/** 解密后的版本元数据（已校验 content 绑定）。 */
export interface VersionMetadataView {
  /** 该版本引用的 content ID。 */
  contentId: string
  /** 明文总大小（字节）。 */
  size: number
  /** 原始修改时间（Unix 秒）。 */
  mtime: number
  /** MIME 类型。 */
  mime: string
}

/** 注册 prepare 结果（仅 WASM → Worker；绝不经过 Comlink）。 */
export interface PreparedRegistrationView {
  display: string
  wrappedMk: Uint8Array
  authKey: Uint8Array
  recoveryWrappedMk: Uint8Array
  recoveryAuthKey: Uint8Array
  kdfParamsJson: string
  kdfSalt: Uint8Array
}

/** 修改密码 prepare 结果（仅 WASM → Worker）。 */
export interface PreparedPasswordChangeView {
  oldAuthKey: Uint8Array
  newAuthKey: Uint8Array
  newKdfParamsJson: string
  newKdfSalt: Uint8Array
  newWrappedMk: Uint8Array
}

/** 恢复重置 prepare 结果（仅 WASM → Worker）。 */
export interface PreparedRecoveryResetView {
  recoveryAuthKey: Uint8Array
  newAuthKey: Uint8Array
  newKdfParamsJson: string
  newKdfSalt: Uint8Array
  newWrappedMk: Uint8Array
}

/** 轮换恢复密钥 prepare 结果（仅 WASM → Worker）。 */
export interface PreparedRecoveryRotationView {
  currentAuthKey: Uint8Array
  recoveryWrappedMk: Uint8Array
  recoveryAuthKey: Uint8Array
  display: string
}

/** 主线程可见的当前用户；不含任何密钥或包裹材料。 */
export interface CurrentUser {
  userId: string
  email: string
  emailVerified: boolean
}

/** 会话列表项（主线程可见，不含令牌）。 */
export interface SessionSummaryView {
  id: string
  createdAt: string
  lastSeenAt: string
  expiresAt: string
  ip: string
  userAgent: string | null
  current: boolean
}

/** 认证请求的错误载荷；`retryAfter` 来自 429 的 Retry-After。 */
export interface AuthErrorPayload {
  code: string
  message: string
  retryAfter?: number
}

/**
 * WASM 密钥库的同步封装。
 *
 * `KeyStore` 由 `crypto-backend.ts` 提供；测试可以替换为假实现来验证 Worker API 契约。
 */
export interface CryptoBackend {
  derivePasswordKeys(password: string, kdfParamsJson: string, salt: Uint8Array): Uint8Array
  unlockWithPassword(wrappedMk: Uint8Array): void
  unlockWithRecovery(recoveryKey: string, wrappedMkRecovery: Uint8Array): void
  validateRecoveryKey(display: string): void
  prepareRegistration(
    password: string,
    kdfParamsJson: string,
    salt: Uint8Array,
  ): PreparedRegistrationView
  preparePasswordChange(
    oldPassword: string,
    currentKdfParamsJson: string,
    currentSalt: Uint8Array,
    newPassword: string,
  ): PreparedPasswordChangeView
  prepareRecoveryReset(
    recoveryKey: string,
    recoveryWrappedMk: Uint8Array,
    newPassword: string,
  ): PreparedRecoveryResetView
  prepareRecoveryRotation(
    currentPassword: string,
    currentKdfParamsJson: string,
    currentSalt: Uint8Array,
  ): PreparedRecoveryRotationView
  commitRegistration(): void
  commitPasswordChange(): void
  commitRecoveryReset(): void
  commitRecoveryRotation(): void
  discardPending(): void
  lock(): void
  isLocked(): boolean
  createNodeKey(nodeId: string): Uint8Array
  loadNodeKey(nodeId: string, wrapped: Uint8Array): void
  encryptNodeMetadata(nodeId: string, name: string): Uint8Array
  decryptNodeMetadata(nodeId: string, ciphertext: Uint8Array): string
  nameToken(parentId: string, name: string): string
  encryptVersionMetadata(
    nodeId: string,
    versionId: string,
    contentId: string,
    size: number,
    mtime: number,
    mime: string,
  ): Uint8Array
  decryptVersionMetadataForContent(
    nodeId: string,
    versionId: string,
    contentId: string,
    ciphertext: Uint8Array,
  ): VersionMetadataView
  chunkCount(size: number): number
  encryptChunk(
    nodeId: string,
    contentId: string,
    index: number,
    count: number,
    isLast: boolean,
    plaintext: Uint8Array,
  ): Uint8Array
  decryptChunk(
    nodeId: string,
    contentId: string,
    index: number,
    count: number,
    isLast: boolean,
    ciphertext: Uint8Array,
  ): Uint8Array
  verifyPlaintextSize(expected: number, actual: number): void
}

/**
 * Comlink 暴露给主线程的高层 API。
 *
 * 约定：**任何方法的入参与返回值都不包含密钥字节**；密钥只存在于 Worker 内的 WASM 线性内存。
 */
export interface CryptoWorkerApi {
  /** 清零并释放 WASM 内的所有密钥。 */
  lock(): Promise<void>
  isLocked(): Promise<boolean>
  /** 为节点生成密钥；返回包裹后的密文。 */
  createNodeKey(nodeId: string): Promise<Uint8Array>
  /** 解包服务端返回的节点密钥并留在 Worker 内。 */
  loadNodeKey(nodeId: string, wrapped: Uint8Array): Promise<void>
  encryptNodeMetadata(nodeId: string, name: string): Promise<Uint8Array>
  decryptNodeMetadata(nodeId: string, ciphertext: Uint8Array): Promise<string>
  nameToken(parentId: string, name: string): Promise<string>
  encryptVersionMetadata(
    nodeId: string,
    versionId: string,
    contentId: string,
    size: number,
    mtime: number,
    mime: string,
  ): Promise<Uint8Array>
  decryptVersionMetadataForContent(
    nodeId: string,
    versionId: string,
    contentId: string,
    ciphertext: Uint8Array,
  ): Promise<VersionMetadataView>
  chunkCount(size: number): Promise<number>
  encryptChunk(
    nodeId: string,
    contentId: string,
    index: number,
    count: number,
    isLast: boolean,
    plaintext: Uint8Array,
  ): Promise<Uint8Array>
  decryptChunk(
    nodeId: string,
    contentId: string,
    index: number,
    count: number,
    isLast: boolean,
    ciphertext: Uint8Array,
  ): Promise<Uint8Array>
  verifyPlaintextSize(expected: number, actual: number): Promise<void>
}

/**
 * Comlink 暴露的认证高层 API。
 *
 * 所有请求在 Worker 内用同源 fetch 发出；主线程只接收用户信息、恢复密钥展示串
 * 与稳定错误码。唯一允许跨 Comlink 的密钥类数据是 `recoveryDisplay`。
 */
export interface AuthWorkerApi {
  /** 准备注册材料并返回恢复密钥展示串；此时不发送注册请求。 */
  register(email: string, password: string): Promise<{ recoveryDisplay: string }>
  /** 用户回填确认后提交注册；成功才 commit KeyStore。 */
  confirmRegistration(): Promise<void>
  /** 取消注册 prepare 并清零待提交对象。 */
  discardRegistration(): Promise<void>
  /** 本地校验恢复密钥（校验位/字符集），不发送 HTTP 请求。 */
  validateRecoveryKey(display: string): Promise<void>
  login(email: string, password: string): Promise<CurrentUser>
  refreshSession(): Promise<CurrentUser | null>
  unlock(password: string): Promise<void>
  changePassword(oldPassword: string, newPassword: string): Promise<void>
  resetWithRecovery(
    email: string,
    recoveryKey: string,
    newPassword: string,
  ): Promise<void>
  prepareRecoveryRotation(currentPassword: string): Promise<{ recoveryDisplay: string }>
  confirmRecoveryRotation(): Promise<void>
  discardRecoveryRotation(): Promise<void>
  logout(): Promise<void>
  verifyEmail(token: string): Promise<void>
  resendVerification(email: string): Promise<void>
  getSessions(): Promise<SessionSummaryView[]>
  revokeSession(sessionId: string): Promise<void>
}

/** Worker 暴露的完整 API。 */
export type WorkerApi = CryptoWorkerApi & AuthWorkerApi
