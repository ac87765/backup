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

/** 注册/首次解锁时返回的包裹后的主密钥（密文，允许跨线程）。 */
export interface WrappedMasterKey {
  wrappedMk: Uint8Array
}

/** 恢复密钥材料（仅 Worker 内部使用；`rkAuth` 不经过 Comlink）。 */
export interface RecoveryMaterialView {
  display: string
  wrappedMkRecovery: Uint8Array
  rkAuth: Uint8Array
}

/**
 * WASM 密钥库的同步封装。
 *
 * `KeyStore` 由 `crypto-backend.ts` 提供；测试可以替换为假实现来验证 Worker API 契约。
 */
export interface CryptoBackend {
  derivePasswordKeys(password: string, kdfParamsJson: string, salt: Uint8Array): Uint8Array
  createMasterKey(): Uint8Array
  unlockWithPassword(wrappedMk: Uint8Array): void
  unlockWithRecovery(recoveryKey: string, wrappedMkRecovery: Uint8Array): void
  createRecoveryKey(): RecoveryMaterialView
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
  /** 派生密码密钥并生成新的 MK；只返回包裹后的 MK（密文）。 */
  createMasterKey(
    password: string,
    kdfParamsJson: string,
    salt: Uint8Array,
  ): Promise<WrappedMasterKey>
  /** 用密码解锁（派生 + 解包 MK）。 */
  unlock(
    password: string,
    kdfParamsJson: string,
    salt: Uint8Array,
    wrappedMk: Uint8Array,
  ): Promise<void>
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
