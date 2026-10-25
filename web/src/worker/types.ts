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
  /** 根文件夹 ID（非密钥；文件浏览器从根开始导航）。 */
  rootNodeId: string
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

/** 节点类型。 */
export type NodeKind = 'file' | 'folder'

/** 主线程可见的节点展示数据；不含包裹密钥、密文或任何密钥材料。 */
export interface FileNodeView {
  id: string
  parentId: string | null
  kind: NodeKind
  /** Worker 内解密后的名称；只驻留内存。 */
  name: string
  /** 文件明文大小；文件夹为 null。 */
  size: number | null
  /** 文件原始修改时间（Unix 秒）；文件夹为 null。 */
  mtime: number | null
  mime: string | null
  rowVersion: number
  currentVersionId: string | null
  createdAt: string
  updatedAt: string
}

/** 回收站顶层项。 */
export interface TrashNodeView extends FileNodeView {
  deletedAt: string
  trashBatchId: string
}

/** 列目录一页。 */
export interface FolderPageView {
  items: FileNodeView[]
  nextCursor: string | null
}

/** 版本展示数据。 */
export interface VersionView {
  id: string
  contentId: string
  size: number
  mtime: number
  mime: string
  isCurrent: boolean
  createdAt: string
}

/** 已用 / 预留 / 总额。 */
export interface UsageView {
  usedBytes: number
  reservedBytes: number
  quotaBytes: number
}

/** 软删除/永久删除结果。 */
export interface DeleteResultView {
  deletedNodes: number
  deletedVersions: number
  deletedContents: number
  freedBytes: number
  queuedObjects: number
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
export type WorkerApi = CryptoWorkerApi & AuthWorkerApi & FileWorkerApi

/**
 * Comlink 暴露的文件管理高层 API。
 *
 * 入参与返回值都只包含节点 ID、解密后的名称/大小/时间等展示数据；
 * 节点密钥、包裹密钥、加密元数据与名称令牌只存在于 Worker 内。
 */
export interface FileWorkerApi {
  /** 列目录；Worker 内解包节点密钥、缓存并解密名称/版本元数据。 */
  listFolder(nodeId: string, cursor?: string): Promise<FolderPageView>
  /** 创建文件夹；Worker 内生成节点 ID/密钥、加密元数据并计算名称令牌。 */
  createFolder(parentId: string, name: string): Promise<FileNodeView>
  /** 重命名；名称令牌按当前父节点重新计算。 */
  renameNode(
    nodeId: string,
    parentId: string,
    name: string,
    rowVersion: number,
  ): Promise<FileNodeView>
  /** 移动；名称令牌按新父节点重新计算。 */
  moveNode(
    nodeId: string,
    newParentId: string,
    name: string,
    rowVersion: number,
  ): Promise<FileNodeView>
  /** 移入回收站。 */
  deleteNode(nodeId: string): Promise<void>
  /** 回收站顶层列表。 */
  listTrash(): Promise<TrashNodeView[]>
  /**
   * 恢复回收站项目。不指定新父节点时使用原位置；目标不可用时客户端
   * 捕获 `target_unavailable` 后让用户选择新位置并重新提交。
   */
  restoreNode(
    nodeId: string,
    newParentId?: string,
    name?: string,
  ): Promise<FileNodeView>
  /** 永久删除回收站项目。 */
  permanentlyDeleteNode(nodeId: string): Promise<DeleteResultView>
  /** 版本列表；Worker 内解密并校验 content 绑定。 */
  listVersions(nodeId: string): Promise<VersionView[]>
  /** 恢复旧版本；Worker 用同一 content_id 为新 version_id 重新加密元数据。 */
  restoreVersion(nodeId: string, sourceVersionId: string): Promise<VersionView>
  /** 已用 / 预留 / 总额。 */
  getUsage(): Promise<UsageView>
}
