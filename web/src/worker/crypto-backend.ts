/**
 * WASM 密钥库的加载与封装。
 *
 * **只允许在 Worker 中 import 本文件**：主线程不得加载 WASM，也不得接触密钥。
 * 生产构建的加载路径由 `initCryptoWasm()` 负责（Vite 把 .wasm 作为静态资源提供）。
 */
import init, {
  ContentHasher as WasmContentHasher,
  KeyStore as WasmKeyStore,
} from '../generated/crypto-wasm/crypto_wasm.js'
import wasmUrl from '../generated/crypto-wasm/crypto_wasm_bg.wasm?url'

import type {
  ContentHasherBackend,
  CryptoBackend,
  PreparedPasswordChangeView,
  PreparedRecoveryResetView,
  PreparedRecoveryRotationView,
  PreparedRegistrationView,
  VersionMetadataView,
} from './types'

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

  unlockWithPassword(wrappedMk: Uint8Array): void {
    this.store.unlock_with_password(wrappedMk)
  }

  unlockWithRecovery(recoveryKey: string, wrappedMkRecovery: Uint8Array): void {
    this.store.unlock_with_recovery(recoveryKey, wrappedMkRecovery)
  }

  validateRecoveryKey(display: string): void {
    this.store.validate_recovery_key(display)
  }

  prepareRegistration(
    password: string,
    kdfParamsJson: string,
    salt: Uint8Array,
  ): PreparedRegistrationView {
    const prepared = this.store.prepare_registration(password, kdfParamsJson, salt)
    const view = {
      display: prepared.display,
      wrappedMk: prepared.wrapped_mk,
      authKey: prepared.auth_key,
      recoveryWrappedMk: prepared.recovery_wrapped_mk,
      recoveryAuthKey: prepared.recovery_auth_key,
      kdfParamsJson: prepared.kdf_params_json,
      kdfSalt: prepared.kdf_salt,
    }
    prepared.free()
    return view
  }

  preparePasswordChange(
    oldPassword: string,
    currentKdfParamsJson: string,
    currentSalt: Uint8Array,
    newPassword: string,
  ): PreparedPasswordChangeView {
    const prepared = this.store.prepare_password_change(
      oldPassword,
      currentKdfParamsJson,
      currentSalt,
      newPassword,
    )
    const view = {
      oldAuthKey: prepared.old_auth_key,
      newAuthKey: prepared.new_auth_key,
      newKdfParamsJson: prepared.new_kdf_params_json,
      newKdfSalt: prepared.new_kdf_salt,
      newWrappedMk: prepared.new_wrapped_mk,
    }
    prepared.free()
    return view
  }

  prepareRecoveryReset(
    recoveryKey: string,
    recoveryWrappedMk: Uint8Array,
    newPassword: string,
  ): PreparedRecoveryResetView {
    const prepared = this.store.prepare_recovery_reset(
      recoveryKey,
      recoveryWrappedMk,
      newPassword,
    )
    const view = {
      recoveryAuthKey: prepared.recovery_auth_key,
      newAuthKey: prepared.new_auth_key,
      newKdfParamsJson: prepared.new_kdf_params_json,
      newKdfSalt: prepared.new_kdf_salt,
      newWrappedMk: prepared.new_wrapped_mk,
    }
    prepared.free()
    return view
  }

  prepareRecoveryRotation(
    currentPassword: string,
    currentKdfParamsJson: string,
    currentSalt: Uint8Array,
  ): PreparedRecoveryRotationView {
    const prepared = this.store.prepare_recovery_rotation(
      currentPassword,
      currentKdfParamsJson,
      currentSalt,
    )
    const view = {
      currentAuthKey: prepared.current_auth_key,
      recoveryWrappedMk: prepared.recovery_wrapped_mk,
      recoveryAuthKey: prepared.recovery_auth_key,
      display: prepared.display,
    }
    prepared.free()
    return view
  }

  commitRegistration(): void {
    this.store.commit_registration()
  }

  commitPasswordChange(): void {
    this.store.commit_password_change()
  }

  commitRecoveryReset(): void {
    this.store.commit_recovery_reset()
  }

  commitRecoveryRotation(): void {
    this.store.commit_recovery_rotation()
  }

  discardPending(): void {
    this.store.discard_pending()
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
    contentSha256: string | null,
  ): Uint8Array {
    return this.store.encrypt_version_metadata(
      nodeId,
      versionId,
      contentId,
      size,
      mtime,
      mime,
      contentSha256,
    )
  }

  createContentHasher(): ContentHasherBackend {
    const hasher = new WasmContentHasher()
    return {
      update: (bytes) => hasher.update(bytes),
      finish: () => {
        try {
          return hasher.finish()
        } finally {
          hasher.free()
        }
      },
    }
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
      contentSha256: view.content_sha256 ?? null,
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

