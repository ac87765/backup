/* eslint-disable @typescript-eslint/only-throw-error -- 跨 Comlink 需要普通对象保留稳定错误码 */
/**
 * Worker 内的上传/下载管线（tasks 8.7–8.8）。
 *
 * 安全边界：
 * - 密钥、明文、密文只在本模块与 WASM/对象存储之间流动；主线程只收到进度计数、
 *   ID 与（显式降级时的）Blob。
 * - 上传块按 4 MiB 分片，内存中同时最多保留 4 个块的明文/密文。
 * - 下载按序解密写出，预取窗口 3；失败时中止文件流，不留下残缺文件。
 */
import type { components } from '../api/schema'
import { validateName } from '../files/name-validation'
import type {
  CryptoBackend,
  DownloadResultView,
  DownloadTargetView,
  FileSystemWritableLike,
  PendingUploadView,
  TransferProgressView,
  TransferStatus,
  TransferWorkerApi,
  UploadResultView,
} from './types'
import type { FileClient } from './file-client'
import {
  backoffDelay,
  isRetryable,
  sleep,
  transferError,
  withRetry,
  type TransferErrorPayload,
} from './retry'
import {
  createUploadStore,
  type UploadFingerprint,
  type UploadRecord,
  type UploadStore,
} from './upload-store'

type UploadSessionResponse = components['schemas']['UploadSessionResponse']
type PresignChunksResponse = components['schemas']['PresignChunksResponse']
type ConfirmChunksResponse = components['schemas']['ConfirmChunksResponse']
type ConfirmedChunksResponse = components['schemas']['ConfirmedChunksResponse']
type DownloadManifest = components['schemas']['DownloadManifest']

/** crypto-core 的块明文大小与帧开销（与 Rust 常量保持一致）。 */
export const CHUNK_PLAINTEXT_SIZE = 4 * 1024 * 1024
export const CHUNK_FRAME_OVERHEAD = 41
export const MAX_BATCH_CHUNKS = 32
export const MAX_UPLOAD_CONCURRENCY = 4
export const DOWNLOAD_PREFETCH = 3
export const MAX_BLOB_DOWNLOAD_BYTES = 2 * 1024 * 1024 * 1024

const MAX_ATTEMPTS = 5

interface Control {
  paused: boolean
  cancelled: boolean
  waiters: Array<() => void>
}

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')
}

function hexToBytes(value: string): Uint8Array {
  if (value.length % 2 !== 0) {
    throw transferError('invalid_response', '服务端返回了非法的十六进制数据')
  }
  const bytes = new Uint8Array(value.length / 2)
  for (let index = 0; index < bytes.length; index += 1) {
    const parsed = Number.parseInt(value.slice(index * 2, index * 2 + 2), 16)
    if (Number.isNaN(parsed)) {
      throw transferError('invalid_response', '服务端返回了非法的十六进制数据')
    }
    bytes[index] = parsed
  }
  return bytes
}

async function sha256Hex(bytes: Uint8Array): Promise<string> {
  const digest = await crypto.subtle.digest(
    'SHA-256',
    bytes as unknown as BufferSource,
  )
  return bytesToHex(new Uint8Array(digest))
}

function fileFingerprint(file: File): UploadFingerprint {
  return {
    name: file.name,
    size: file.size,
    lastModified: file.lastModified,
  }
}

function fileMtime(file: File): number {
  return Math.floor(file.lastModified / 1000)
}

function fileMime(file: File): string {
  return file.type || 'application/octet-stream'
}

function chunkCountFor(backend: CryptoBackend, size: number): number {
  const count = backend.chunkCount(size)
  if (!Number.isSafeInteger(count) || count < 1) {
    throw transferError('invalid_chunk_layout', '无法计算分块数量')
  }
  return count
}

/** 按并发上限执行异步任务；用于上传的 PUT 阶段。 */
async function mapWithConcurrency<T>(
  items: T[],
  limit: number,
  operation: (item: T) => Promise<void>,
): Promise<void> {
  let next = 0
  const workerCount = Math.min(limit, items.length)
  const workers = Array.from({ length: workerCount }, async () => {
    for (;;) {
      const index = next
      next += 1
      if (index >= items.length) {
        return
      }
      await operation(items[index])
    }
  })
  await Promise.all(workers)
}

function chunkBytes(index: number, file: File): { start: number; end: number } {
  const start = index * CHUNK_PLAINTEXT_SIZE
  return { start, end: Math.min(file.size, start + CHUNK_PLAINTEXT_SIZE) }
}

function plaintextBytesForCiphertext(declaredSize: number): number {
  return Math.max(0, declaredSize - CHUNK_FRAME_OVERHEAD)
}

interface DownloadSink {
  write(bytes: Uint8Array): Promise<void>
  finish(): Promise<Blob | null>
  abort(): Promise<void>
}

export class TransferClient implements TransferWorkerApi {
  private readonly backend: Promise<CryptoBackend>
  private readonly fileClient: FileClient
  private readonly store: UploadStore
  private readonly fetchImpl: typeof fetch
  private readonly getUserId: () => string | null
  private readonly controls = new Map<string, Control>()

  constructor(
    backend: Promise<CryptoBackend>,
    fileClient: FileClient,
    options: {
      store?: UploadStore
      fetchImpl?: typeof fetch
      getUserId?: () => string | null
    } = {},
  ) {
    this.backend = backend
    this.fileClient = fileClient
    this.store = options.store ?? createUploadStore()
    this.fetchImpl =
      options.fetchImpl ?? ((input, init) => fetch(input, init))
    this.getUserId = options.getUserId ?? (() => null)
  }

  async uploadFile(
    parentId: string,
    file: File,
    onProgress?: (progress: TransferProgressView) => void,
  ): Promise<UploadResultView> {
    this.validateName(file.name)
    const backend = await this.unlockedBackend()
    const existing = await this.store.findMatching(
      'new_file',
      parentId,
      fileFingerprint(file),
    )
    if (existing) {
      return this.resumeInternal(existing, file, onProgress)
    }
    return this.startNewFile(backend, parentId, file, onProgress)
  }

  async uploadNewVersion(
    nodeId: string,
    file: File,
    onProgress?: (progress: TransferProgressView) => void,
  ): Promise<UploadResultView> {
    this.validateName(file.name)
    const backend = await this.unlockedBackend()
    const existing = await this.store.findMatching(
      'new_version',
      null,
      fileFingerprint(file),
      nodeId,
    )
    if (existing) {
      return this.resumeInternal(existing, file, onProgress)
    }
    return this.startNewVersion(backend, nodeId, file, onProgress)
  }

  async resumePendingUpload(
    sessionId: string,
    file: File,
    onProgress?: (progress: TransferProgressView) => void,
  ): Promise<UploadResultView> {
    const record = await this.store.get(sessionId)
    if (!record) {
      throw transferError('upload_session_not_found', '找不到待续传的上传会话')
    }
    if (
      record.name !== file.name ||
      record.size !== file.size ||
      record.lastModified !== file.lastModified
    ) {
      throw transferError(
        'fingerprint_mismatch',
        '所选文件与待续传会话的文件指纹不一致',
      )
    }
    return this.resumeInternal(record, file, onProgress)
  }

  async listPendingUploads(): Promise<PendingUploadView[]> {
    const records = await this.store.list()
    return records.map((record) => ({
      sessionId: record.sessionId,
      userId: record.userId,
      mode: record.mode,
      parentId: record.parentId,
      nodeId: record.nodeId,
      contentId: record.contentId,
      versionId: record.versionId,
      name: record.name,
      size: record.size,
      lastModified: record.lastModified,
      status: record.status,
    }))
  }

  /**
   * 退出登录时清理该用户的续传记录：先尽力中止服务端会话，再删除本地记录，
   * 并取消该用户正在进行的传输。其他用户的记录保持不变。
   */
  async clearPendingUploads(userId: string): Promise<void> {
    for (const control of this.controls.values()) {
      control.cancelled = true
      this.wake(control)
    }
    const records = await this.store.list()
    for (const record of records) {
      if (record.userId !== userId) {
        continue
      }
      try {
        await this.request(
          `/api/v1/upload-sessions/${encodeURIComponent(record.sessionId)}/abort`,
          { method: 'POST' },
        )
      } catch {
        // 会话可能已关闭或网络不可用；本地记录仍必须清理，服务端会按过期清理。
      }
      await this.store.delete(record.sessionId)
    }
  }

  async pauseUpload(sessionId: string): Promise<void> {
    const control = this.control(sessionId)
    control.paused = true
    const record = await this.store.get(sessionId)
    if (record) {
      await this.store.put({ ...record, status: 'paused' })
    }
  }

  async resumeUpload(sessionId: string): Promise<void> {
    const control = this.control(sessionId)
    control.paused = false
    this.wake(control)
    const record = await this.store.get(sessionId)
    if (record) {
      await this.store.put({ ...record, status: 'uploading' })
    }
  }

  async cancelUpload(sessionId: string): Promise<void> {
    const control = this.control(sessionId)
    control.cancelled = true
    this.wake(control)
    try {
      await this.request(
        `/api/v1/upload-sessions/${encodeURIComponent(sessionId)}/abort`,
        { method: 'POST' },
      )
    } catch (error) {
      const code = (error as TransferErrorPayload).code
      if (
        code !== 'not_found' &&
        code !== 'upload_session_closed' &&
        code !== 'unauthenticated'
      ) {
        throw error
      }
    } finally {
      await this.store.delete(sessionId)
    }
  }

  async downloadVersion(
    nodeId: string,
    versionId: string,
    downloadId: string,
    fileName: string,
    target: DownloadTargetView,
    onProgress?: (progress: TransferProgressView) => void,
  ): Promise<DownloadResultView> {
    const backend = await this.unlockedBackend()
    const control = this.control(downloadId)
    this.emit(onProgress, {
      transferId: downloadId,
      direction: 'download',
      fileName,
      status: 'downloading',
      completedChunks: 0,
      totalChunks: 0,
      completedBytes: 0,
      totalBytes: 0,
    })

    let sink: DownloadSink | null = null
    try {
      const manifest = await this.request<DownloadManifest>(
        `/api/v1/nodes/${encodeURIComponent(nodeId)}/versions/${encodeURIComponent(versionId)}/download`,
        { method: 'GET' },
      )
      const wrapped =
        manifest.wrapped_key === null || manifest.wrapped_key === undefined
          ? this.fileClient.wrappedKeyFor(nodeId)
          : hexToBytes(manifest.wrapped_key)
      if (!wrapped) {
        throw transferError('node_key_missing', '节点密钥尚未加载，请刷新后重试')
      }
      backend.loadNodeKey(nodeId, wrapped)
      this.fileClient.rememberWrappedKey(nodeId, wrapped)

      let metadata
      try {
        metadata = backend.decryptVersionMetadataForContent(
          nodeId,
          versionId,
          manifest.content_id,
          hexToBytes(manifest.encrypted_meta),
        )
      } catch {
        throw transferError('integrity_error', '版本元数据校验失败')
      }
      const expectedChannels = chunkCountFor(backend, metadata.size)
      if (expectedChannels !== manifest.chunk_count) {
        throw transferError('integrity_error', '版本元数据与块数量不一致')
      }
      if (
        target.kind === 'blob' &&
        metadata.size > MAX_BLOB_DOWNLOAD_BYTES
      ) {
        throw transferError(
          'file_too_large',
          '当前浏览器只能以内存方式下载，文件不能超过 2 GiB',
        )
      }

      const progress = (
        status: TransferStatus,
        completedChunks: number,
        completedBytes: number,
        error?: TransferErrorPayload,
      ) =>
        this.emit(onProgress, {
          transferId: downloadId,
          direction: 'download',
          fileName,
          status,
          completedChunks,
          totalChunks: manifest.chunk_count,
          completedBytes,
          totalBytes: metadata.size,
          ...(error
            ? { errorCode: error.code, errorMessage: error.message }
            : {}),
        })

      sink = await this.createSink(target, metadata.mime)
      const chunks = new Map<number, components['schemas']['DownloadChunk']>()
      for (const chunk of manifest.chunks) {
        chunks.set(chunk.idx, chunk)
      }

      const prefetched = new Map<number, Promise<Uint8Array>>()
      const ensureFetch = (index: number): Promise<Uint8Array> => {
        const previous = prefetched.get(index)
        if (previous) {
          return previous
        }
        const chunk = chunks.get(index)
        if (!chunk) {
          throw transferError('integrity_error', '下载清单缺少块')
        }
        const pending = this.fetchChunk(chunk.url)
        // 防止靠后的预取错误在按序 await 之前变成未处理拒绝。
        void pending.catch(() => undefined)
        prefetched.set(index, pending)
        return pending
      }

      let completedBytes = 0
      for (let index = 0; index < manifest.chunk_count; index += 1) {
        await this.checkpoint(control, () => progress('paused', index, completedBytes))
        for (
          let ahead = index;
          ahead < Math.min(manifest.chunk_count, index + DOWNLOAD_PREFETCH);
          ahead += 1
        ) {
          void ensureFetch(ahead)
        }
        const pendingCiphertext = prefetched.get(index)
        if (!pendingCiphertext) {
          throw transferError('integrity_error', '下载清单缺少块')
        }
        const ciphertext = await pendingCiphertext
        prefetched.delete(index)
        let plaintext: Uint8Array
        try {
          plaintext = backend.decryptChunk(
            nodeId,
            manifest.content_id,
            index,
            manifest.chunk_count,
            index === manifest.chunk_count - 1,
            ciphertext,
          )
        } catch {
          throw transferError('integrity_error', '块解密认证失败')
        }
        assertChunkLength(plaintext, metadata.size, index, manifest.chunk_count)
        await sink.write(plaintext)
        completedBytes += plaintext.byteLength
        progress('downloading', index + 1, completedBytes)
      }

      try {
        backend.verifyPlaintextSize(metadata.size, completedBytes)
      } catch {
        throw transferError('integrity_error', '解密后的明文总大小不符')
      }
      const blob = await sink.finish()
      progress('completed', manifest.chunk_count, completedBytes)
      return { blob }
    } catch (error) {
      const payload = error as TransferErrorPayload
      const cancelled = control.cancelled || payload.code === 'cancelled'
      if (sink) {
        try {
          await sink.abort()
        } catch {
          // 中止失败不应覆盖原始错误。
        }
      }
      const status: TransferStatus = cancelled ? 'cancelled' : 'failed'
      this.emit(onProgress, {
        transferId: downloadId,
        direction: 'download',
        fileName,
        status,
        completedChunks: 0,
        totalChunks: 0,
        completedBytes: 0,
        totalBytes: 0,
        errorCode: cancelled ? 'cancelled' : payload.code,
        errorMessage: cancelled ? '传输已取消' : payload.message,
      })
      if (cancelled) {
        throw transferError('cancelled', '传输已取消')
      }
      throw error
    } finally {
      this.controls.delete(downloadId)
    }
  }

  pauseDownload(downloadId: string): Promise<void> {
    this.control(downloadId).paused = true
    return Promise.resolve()
  }

  resumeDownload(downloadId: string): Promise<void> {
    const control = this.control(downloadId)
    control.paused = false
    this.wake(control)
    return Promise.resolve()
  }

  cancelDownload(downloadId: string): Promise<void> {
    const control = this.control(downloadId)
    control.cancelled = true
    this.wake(control)
    return Promise.resolve()
  }

  private validateName(name: string): void {
    const message = validateName(name)
    if (message) {
      throw transferError('invalid_name', message)
    }
  }

  private async unlockedBackend(): Promise<CryptoBackend> {
    const backend = await this.backend
    if (backend.isLocked()) {
      throw transferError('locked', '密钥库已锁定，请先解锁')
    }
    return backend
  }

  private async startNewFile(
    backend: CryptoBackend,
    parentId: string,
    file: File,
    onProgress?: (progress: TransferProgressView) => void,
  ): Promise<UploadResultView> {
    const nodeId = crypto.randomUUID()
    const contentId = crypto.randomUUID()
    const versionId = crypto.randomUUID()
    const wrapped = backend.createNodeKey(nodeId)
    this.fileClient.rememberWrappedKey(nodeId, wrapped)
    const encryptedMeta = backend.encryptNodeMetadata(nodeId, file.name)
    const nameToken = backend.nameToken(parentId, file.name)
    const count = chunkCountFor(backend, file.size)
    const declaredSize = file.size + count * CHUNK_FRAME_OVERHEAD
    const versionMeta = backend.encryptVersionMetadata(
      nodeId,
      versionId,
      contentId,
      file.size,
      fileMtime(file),
      fileMime(file),
    )
    const session = await this.request<UploadSessionResponse>(
      '/api/v1/upload-sessions',
      {
        method: 'POST',
        body: JSON.stringify({
          content_id: contentId,
          kind: 'new_file',
          node_id: nodeId,
          parent_id: parentId,
          wrapped_key: bytesToHex(wrapped),
          encrypted_meta: bytesToHex(encryptedMeta),
          name_token: nameToken,
          version_id: versionId,
          version_meta: bytesToHex(versionMeta),
          chunk_count: count,
          declared_size: declaredSize,
        }),
      },
    )
    this.verifySession(session, {
      contentId,
      nodeId,
      versionId,
      count,
      declaredSize,
    })
    const record: UploadRecord = {
      sessionId: session.id,
      userId: this.getUserId(),
      mode: 'new_file',
      parentId,
      nodeId,
      contentId,
      versionId,
      name: file.name,
      size: file.size,
      lastModified: file.lastModified,
      mime: fileMime(file),
      mtime: fileMtime(file),
      status: 'uploading',
      createdAt: Date.now(),
    }
    await this.store.put(record)
    return this.runSession(record, file, session, onProgress)
  }

  private async startNewVersion(
    backend: CryptoBackend,
    nodeId: string,
    file: File,
    onProgress?: (progress: TransferProgressView) => void,
  ): Promise<UploadResultView> {
    const wrapped = this.fileClient.wrappedKeyFor(nodeId)
    if (!wrapped) {
      throw transferError(
        'node_key_missing',
        '节点密钥尚未加载，请刷新文件夹后重试',
      )
    }
    backend.loadNodeKey(nodeId, wrapped)
    const contentId = crypto.randomUUID()
    const versionId = crypto.randomUUID()
    const count = chunkCountFor(backend, file.size)
    const declaredSize = file.size + count * CHUNK_FRAME_OVERHEAD
    const versionMeta = backend.encryptVersionMetadata(
      nodeId,
      versionId,
      contentId,
      file.size,
      fileMtime(file),
      fileMime(file),
    )
    const session = await this.request<UploadSessionResponse>(
      '/api/v1/upload-sessions',
      {
        method: 'POST',
        body: JSON.stringify({
          content_id: contentId,
          kind: 'new_version',
          node_id: nodeId,
          version_id: versionId,
          version_meta: bytesToHex(versionMeta),
          chunk_count: count,
          declared_size: declaredSize,
        }),
      },
    )
    this.verifySession(session, {
      contentId,
      nodeId,
      versionId,
      count,
      declaredSize,
    })
    const record: UploadRecord = {
      sessionId: session.id,
      userId: this.getUserId(),
      mode: 'new_version',
      parentId: null,
      nodeId,
      contentId,
      versionId,
      name: file.name,
      size: file.size,
      lastModified: file.lastModified,
      mime: fileMime(file),
      mtime: fileMtime(file),
      status: 'uploading',
      createdAt: Date.now(),
    }
    await this.store.put(record)
    return this.runSession(record, file, session, onProgress)
  }

  private verifySession(
    session: UploadSessionResponse,
    expected: {
      contentId: string
      nodeId: string
      versionId: string
      count: number
      declaredSize: number
    },
  ): void {
    if (
      session.content_id !== expected.contentId ||
      session.node_id !== expected.nodeId ||
      session.version_id !== expected.versionId ||
      session.chunk_count !== expected.count ||
      session.declared_size !== expected.declaredSize
    ) {
      throw transferError('invalid_response', '上传会话与请求不一致')
    }
  }

  private async resumeInternal(
    record: UploadRecord,
    file: File,
    onProgress?: (progress: TransferProgressView) => void,
  ): Promise<UploadResultView> {
    if (
      record.name !== file.name ||
      record.size !== file.size ||
      record.lastModified !== file.lastModified
    ) {
      throw transferError(
        'fingerprint_mismatch',
        '所选文件与待续传会话的文件指纹不一致',
      )
    }
    const backend = await this.unlockedBackend()
    const session = await this.request<UploadSessionResponse>(
      `/api/v1/upload-sessions/${encodeURIComponent(record.sessionId)}`,
      { method: 'GET' },
    )
    if (session.status === 'committed') {
      await this.store.delete(record.sessionId)
      const result: UploadResultView = {
        nodeId: session.node_id,
        versionId: session.version_id,
        contentId: session.content_id,
        name: record.name,
        size: record.size,
      }
      this.emit(onProgress, {
        transferId: record.sessionId,
        direction: 'upload',
        fileName: record.name,
        status: 'completed',
        completedChunks: session.chunk_count,
        totalChunks: session.chunk_count,
        completedBytes: file.size,
        totalBytes: file.size,
      })
      return result
    }
    if (session.status !== 'open') {
      throw transferError('upload_session_closed', '上传会话已关闭')
    }
    const count = chunkCountFor(backend, file.size)
    const declaredSize = file.size + count * CHUNK_FRAME_OVERHEAD
    this.verifySession(session, {
      contentId: record.contentId,
      nodeId: record.nodeId,
      versionId: record.versionId,
      count,
      declaredSize,
    })
    const wrapped =
      session.wrapped_key === null || session.wrapped_key === undefined
        ? this.fileClient.wrappedKeyFor(record.nodeId)
        : hexToBytes(session.wrapped_key)
    if (!wrapped) {
      throw transferError('node_key_missing', '无法恢复节点密钥')
    }
    backend.loadNodeKey(record.nodeId, wrapped)
    this.fileClient.rememberWrappedKey(record.nodeId, wrapped)
    return this.runSession(record, file, session, onProgress)
  }

  private async runSession(
    record: UploadRecord,
    file: File,
    session: UploadSessionResponse,
    onProgress?: (progress: TransferProgressView) => void,
  ): Promise<UploadResultView> {
    const backend = await this.unlockedBackend()
    const control = this.control(record.sessionId)
    const count = session.chunk_count
    const totalBytes = file.size
    const confirmedResponse = await this.request<ConfirmedChunksResponse>(
      `/api/v1/upload-sessions/${encodeURIComponent(record.sessionId)}/chunks`,
      { method: 'GET' },
    )
    const confirmed = new Set<number>()
    let completedChunks = 0
    let completedBytes = 0
    for (const chunk of confirmedResponse.items) {
      confirmed.add(chunk.idx)
      completedChunks += 1
      completedBytes += plaintextBytesForCiphertext(chunk.declared_size)
    }

    const progress = (
      status: TransferStatus,
      error?: TransferErrorPayload,
    ) =>
      this.emit(onProgress, {
        transferId: record.sessionId,
        direction: 'upload',
        fileName: record.name,
        status,
        completedChunks,
        totalChunks: count,
        completedBytes,
        totalBytes,
        ...(error
          ? { errorCode: error.code, errorMessage: error.message }
          : {}),
      })

    const missing: number[] = []
    for (let index = 0; index < count; index += 1) {
      if (!confirmed.has(index)) {
        missing.push(index)
      }
    }
    progress('uploading')

    try {
      for (let offset = 0; offset < missing.length; offset += MAX_BATCH_CHUNKS) {
        await this.checkpoint(control, () => progress('paused'))
        const batch = missing.slice(offset, offset + MAX_BATCH_CHUNKS)
        const presigned = await this.presignBatch(record.sessionId, batch)
        const successful: ConfirmChunkItemInput[] = []
        const failed: number[] = []
        let lastError: TransferErrorPayload | null = null

        await mapWithConcurrency(batch, MAX_UPLOAD_CONCURRENCY, async (index) => {
          await this.checkpoint(control, () => progress('paused'))
          const item = presigned.get(index)
          if (
            !item ||
            !item.ok ||
            !item.url ||
            item.expected_size === undefined ||
            item.expected_size === null
          ) {
            failed.push(index)
            lastError = transferError('presign_failed', '预签名失败')
            return
          }
          try {
            const uploaded = await this.uploadOneChunk(
              record,
              file,
              backend,
              index,
              count,
              item.url,
              item.expected_size,
            )
            successful.push(uploaded)
          } catch (error) {
            failed.push(index)
            lastError = error as TransferErrorPayload
          }
        })

        if (successful.length > 0) {
          const confirmedItems = await this.confirmBatch(
            record.sessionId,
            successful,
          )
          for (const result of confirmedItems) {
            if (
              result.ok &&
              result.declared_size !== undefined &&
              result.declared_size !== null
            ) {
              completedChunks += 1
              completedBytes += plaintextBytesForCiphertext(result.declared_size)
            } else {
              failed.push(result.idx)
              lastError = transferError(
                result.code ?? 'confirm_failed',
                result.message ?? '块确认失败',
              )
            }
          }
          progress('uploading')
        }

        if (failed.length > 0) {
          throw transferError(
            lastError?.code ?? 'upload_failed',
            lastError?.message ?? '部分块上传失败',
            { missingChunks: [...new Set(failed)].sort((a, b) => a - b) },
          )
        }
      }

      await this.checkpoint(control, () => progress('paused'))
      const result = await this.request<{
        session_id: string
        status: string
        node_id: string
        version_id: string
        content_id: string
        ciphertext_size: number
        chunk_count: number
        committed_at: string
      }>(
        `/api/v1/upload-sessions/${encodeURIComponent(record.sessionId)}/commit`,
        { method: 'POST' },
      )
      if (result.status !== 'committed') {
        throw transferError('upload_failed', '提交后会话状态异常')
      }
      await this.store.delete(record.sessionId)
      completedChunks = count
      completedBytes = totalBytes
      progress('completed')
      return {
        nodeId: result.node_id,
        versionId: result.version_id,
        contentId: result.content_id,
        name: record.name,
        size: record.size,
      }
    } catch (error) {
      const payload = error as TransferErrorPayload
      if (control.cancelled || payload.code === 'cancelled') {
        progress('cancelled')
        throw transferError('cancelled', '传输已取消')
      }
      const current = await this.store.get(record.sessionId)
      if (current) {
        await this.store.put({ ...current, status: 'failed' })
      }
      progress('failed', payload)
      throw error
    } finally {
      this.controls.delete(record.sessionId)
    }
  }

  private async uploadOneChunk(
    record: UploadRecord,
    file: File,
    backend: CryptoBackend,
    index: number,
    count: number,
    initialUrl: string,
    expectedSize: number,
  ): Promise<ConfirmChunkItemInput> {
    const { start, end } = chunkBytes(index, file)
    const plaintext = new Uint8Array(await file.slice(start, end).arrayBuffer())
    const ciphertext = backend.encryptChunk(
      record.nodeId,
      record.contentId,
      index,
      count,
      index === count - 1,
      plaintext,
    )
    plaintext.fill(0)
    if (ciphertext.byteLength !== expectedSize) {
      throw transferError('chunk_size_mismatch', '加密后的块大小与预期不一致')
    }
    const hash = await sha256Hex(ciphertext)
    let url = initialUrl
    let attempt = 0
    let resigns = 0
    for (;;) {
      let response: Response | null
      try {
        response = await this.fetchImpl(url, {
          method: 'PUT',
          body: ciphertext as unknown as BodyInit,
        })
      } catch {
        response = null
      }
      if (response?.ok) {
        return {
          idx: index,
          declared_size: ciphertext.byteLength,
          sha256: hash,
        }
      }
      if (response?.status === 403 && resigns < MAX_ATTEMPTS) {
        resigns += 1
        const presigned = await this.presignBatch(record.sessionId, [index])
        const item = presigned.get(index)
        if (!item?.url) {
          throw transferError('presign_failed', 'URL 过期后重新预签名失败')
        }
        url = item.url
        continue
      }
      const error = response
        ? await responseError(response)
        : transferError('network_error', '上传块时网络中断')
      attempt += 1
      if (attempt >= MAX_ATTEMPTS || !isRetryable(error)) {
        throw error
      }
      await sleep(backoffDelay(attempt - 1))
    }
  }

  private async presignBatch(
    sessionId: string,
    indices: number[],
  ): Promise<
    Map<
      number,
      components['schemas']['PresignChunkItemResult']
    >
  > {
    const response = await this.request<PresignChunksResponse>(
      `/api/v1/upload-sessions/${encodeURIComponent(sessionId)}/chunks/presign`,
      {
        method: 'POST',
        body: JSON.stringify({ indices }),
      },
    )
    return new Map(response.items.map((item) => [item.idx, item]))
  }

  private async confirmBatch(
    sessionId: string,
    chunks: ConfirmChunkItemInput[],
  ): Promise<components['schemas']['ConfirmChunkItemResult'][]> {
    const response = await this.request<ConfirmChunksResponse>(
      `/api/v1/upload-sessions/${encodeURIComponent(sessionId)}/chunks/confirm`,
      {
        method: 'POST',
        body: JSON.stringify({
          chunks: chunks.map((chunk) => ({
            idx: chunk.idx,
            declared_size: chunk.declared_size,
            sha256: chunk.sha256,
          })),
        }),
      },
    )
    return response.items
  }

  private async fetchChunk(url: string): Promise<Uint8Array> {
    return withRetry(async () => {
      let response: Response
      try {
        response = await this.fetchImpl(url, { method: 'GET' })
      } catch {
        throw transferError('network_error', '下载块时网络中断')
      }
      if (!response.ok) {
        throw await responseError(response)
      }
      return new Uint8Array(await response.arrayBuffer())
    })
  }

  private async createSink(
    target: DownloadTargetView,
    mime: string,
  ): Promise<DownloadSink> {
    if (target.kind === 'file-system') {
      const writable: FileSystemWritableLike =
        await target.handle.createWritable()
      return {
        write: (bytes) =>
          writable.write(bytes as unknown as BufferSource),
        finish: async () => {
          await writable.close()
          return null
        },
        abort: () => writable.abort(),
      }
    }
    if (target.kind === 'service-worker') {
      const channel = new BroadcastChannel(target.downloadId)
      return {
        write: (bytes) => {
          const copy = bytes.slice()
          channel.postMessage({ type: 'chunk', data: copy.buffer })
          return Promise.resolve()
        },
        finish: () => {
          channel.postMessage({ type: 'end' })
          channel.close()
          return Promise.resolve(null)
        },
        abort: () => {
          channel.postMessage({ type: 'error', message: '下载失败' })
          channel.close()
          return Promise.resolve()
        },
      }
    }
    const parts: BlobPart[] = []
    return {
      write: (bytes) => {
        parts.push(bytes.slice())
        return Promise.resolve()
      },
      finish: () => Promise.resolve(new Blob(parts, { type: mime })),
      abort: () => {
        parts.length = 0
        return Promise.resolve()
      },
    }
  }

  private control(id: string): Control {
    let control = this.controls.get(id)
    if (!control) {
      control = { paused: false, cancelled: false, waiters: [] }
      this.controls.set(id, control)
    }
    return control
  }

  private wake(control: Control): void {
    const waiters = control.waiters.splice(0)
    for (const waiter of waiters) {
      waiter()
    }
  }

  private async checkpoint(
    control: Control,
    onPaused: () => void,
  ): Promise<void> {
    while (control.paused && !control.cancelled) {
      onPaused()
      await new Promise<void>((resolve) => {
        control.waiters.push(resolve)
      })
    }
    if (control.cancelled) {
      throw transferError('cancelled', '传输已取消')
    }
  }

  private emit(
    callback: ((progress: TransferProgressView) => void) | undefined,
    progress: TransferProgressView,
  ): void {
    try {
      callback?.(progress)
    } catch {
      // UI 回调失败不能中断传输。
    }
  }

  private async request<T>(
    path: string,
    init: { method: string; body?: string },
  ): Promise<T> {
    return withRetry(async () => {
      let response: Response
      try {
        response = await this.fetchImpl(path, {
          method: init.method,
          credentials: 'same-origin',
          headers: {
            'X-Requested-With': 'backup-web',
            ...(init.body ? { 'Content-Type': 'application/json' } : {}),
          },
          body: init.body,
        })
      } catch {
        throw transferError('network_error', '网络请求失败，请稍后重试')
      }
      const text = await response.text()
      let payload: unknown
      if (text) {
        try {
          payload = JSON.parse(text) as unknown
        } catch {
          payload = undefined
        }
      }
      if (!response.ok) {
        throw await responseError(response, payload)
      }
      return payload as T
    })
  }
}

interface ConfirmChunkItemInput {
  idx: number
  declared_size: number
  sha256: string
}

function assertChunkLength(
  plaintext: Uint8Array,
  totalSize: number,
  index: number,
  count: number,
): void {
  const expected =
    index === count - 1
      ? totalSize - index * CHUNK_PLAINTEXT_SIZE
      : CHUNK_PLAINTEXT_SIZE
  if (plaintext.byteLength !== expected) {
    throw transferError('integrity_error', '块明文大小不符合分块布局')
  }
}

async function responseError(
  response: Response,
  payload?: unknown,
): Promise<TransferErrorPayload> {
  let body = payload
  if (body === undefined) {
    try {
      body = await response.json()
    } catch {
      body = undefined
    }
  }
  const error = body as { code?: unknown; message?: unknown } | undefined
  const retryAfter = Number(response.headers.get('retry-after'))
  return transferError(
    typeof error?.code === 'string' ? error.code : 'http_error',
    typeof error?.message === 'string' ? error.message : '请求失败',
    {
      status: response.status,
      ...(Number.isFinite(retryAfter) && retryAfter > 0
        ? { retryAfter }
        : {}),
    },
  )
}
