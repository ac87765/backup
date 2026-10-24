/**
 * WASM 密钥库的加载与封装。
 *
 * **只允许在 Worker 中 import 本文件**：主线程不得加载 WASM，也不得接触密钥。
 * 生产构建的加载路径由 `initCryptoWasm()` 负责（Vite 把 .wasm 作为静态资源提供）。
 */
import init, { KeyStore as WasmKeyStore } from '../generated/crypto-wasm/crypto_wasm.js'
import wasmUrl from '../generated/crypto-wasm/crypto_wasm_bg.wasm?url'

import type { CryptoBackend, RecoveryMaterialView, VersionMetadataView } from './types'

let initialized: Promise<void> | undefined

/** 初始化 WASM 模块（幂等）。 */
export function initCryptoWasm(): Promise<void> {
  if (!initialized) {
    initialized = init({ module_or_path: wasmUrl }).then(() => undefined)
  }
  return initialized
}

/** `crypto-core` 的 WASM `KeyStore` 到 Worker API 的同步适配层。 */
export class WasmCryptoBackend implements CryptoBackend {
  private readonly store = new WasmKeyStore()

  derivePasswordKeys(
    password: string,
    kdfParamsJson: string,
    salt: Uint8Array,
  ): Uint8Array {
    return this.store.derive_password_keys(password, kdfParamsJson, salt)
  }

  createMasterKey(): Uint8Array {
    return this.store.create_master_key()
  }

  unlockWithPassword(wrappedMk: Uint8Array): void {
    this.store.unlock_with_password(wrappedMk)
  }

  unlockWithRecovery(recoveryKey: string, wrappedMkRecovery: Uint8Array): void {
    this.store.unlock_with_recovery(recoveryKey, wrappedMkRecovery)
  }

  createRecoveryKey(): RecoveryMaterialView {
    const material = this.store.create_recovery_key()
    return {
      display: material.display,
      wrappedMkRecovery: Uint8Array.from(material.wrapped_mk_recovery),
      rkAuth: Uint8Array.from(material.rk_auth),
    }
  }

  lock(): void {
    this.store.lock()
  }

  isLocked(): boolean {
    return this.store.is_locked()
  }

  createNodeKey(nodeId: string): Uint8Array {
    return this.store.generate_node_key(nodeId)
  }

  loadNodeKey(nodeId: string, wrapped: Uint8Array): void {
    this.store.load_node_key(nodeId, wrapped)
  }

  encryptNodeMetadata(nodeId: string, name: string): Uint8Array {
    return this.store.encrypt_node_metadata(nodeId, name)
  }

  decryptNodeMetadata(nodeId: string, ciphertext: Uint8Array): string {
    return this.store.decrypt_node_metadata(nodeId, ciphertext)
  }

  nameToken(parentId: string, name: string): string {
    return this.store.name_token(parentId, name)
  }

  encryptVersionMetadata(
    nodeId: string,
    versionId: string,
    contentId: string,
    size: number,
    mtime: number,
    mime: string,
  ): Uint8Array {
    return this.store.encrypt_version_metadata(
      nodeId,
      versionId,
      contentId,
      size,
      mtime,
      mime,
    )
  }

  decryptVersionMetadataForContent(
    nodeId: string,
    versionId: string,
    contentId: string,
    ciphertext: Uint8Array,
  ): VersionMetadataView {
    const view = this.store.decrypt_version_metadata_for_content(
      nodeId,
      versionId,
      contentId,
      ciphertext,
    )
    return {
      contentId: view.content_id,
      size: view.size,
      mtime: view.mtime,
      mime: view.mime,
    }
  }

  chunkCount(size: number): number {
    return this.store.chunk_count(size)
  }

  encryptChunk(
    nodeId: string,
    contentId: string,
    index: number,
    count: number,
    isLast: boolean,
    plaintext: Uint8Array,
  ): Uint8Array {
    return this.store.encrypt_chunk(
      nodeId,
      contentId,
      index,
      count,
      isLast,
      plaintext,
    )
  }

  decryptChunk(
    nodeId: string,
    contentId: string,
    index: number,
    count: number,
    isLast: boolean,
    ciphertext: Uint8Array,
  ): Uint8Array {
    return this.store.decrypt_chunk(
      nodeId,
      contentId,
      index,
      count,
      isLast,
      ciphertext,
    )
  }

  verifyPlaintextSize(expected: number, actual: number): void {
    this.store.verify_plaintext_size(expected, actual)
  }
}

