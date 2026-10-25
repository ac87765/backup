/* eslint-disable @typescript-eslint/only-throw-error -- Comlink 需要普通对象才能保留稳定错误码 */
/**
 * Worker 内的文件管理客户端（task 7.8）。
 *
 * - 所有请求由 Worker 用同源 fetch 发出，带 `X-Requested-With: backup-web`；
 * - 节点密钥解包后只留在 WASM KeyStore 内；主线程只拿到解密后的展示数据；
 * - 创建节点 ID、节点密钥、加密元数据与名称令牌全部在 Worker 内完成。
 */
import type { components } from '../api/schema'
import { validateName } from '../files/name-validation'
import type {
  AuthErrorPayload,
  CryptoBackend,
  DeleteResultView,
  FileNodeView,
  FileWorkerApi,
  FolderPageView,
  NodeKind,
  TrashNodeView,
  UsageView,
  VersionView,
} from './types'

type NodeSummary = components['schemas']['NodeSummary']
type VersionSummary = components['schemas']['VersionSummary']
type ChildrenResponse = components['schemas']['ChildrenResponse']
type TrashResponse = components['schemas']['TrashResponse']
type VersionsResponse = components['schemas']['VersionsResponse']
type UsageResponse = components['schemas']['UsageResponse']
type PermanentDeleteResponse = components['schemas']['PermanentDeleteResponse']

function fileError(code: string, message: string): AuthErrorPayload {
  return { code, message }
}

function bytesToHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')
}

function hexToBytes(hex: string): Uint8Array {
  if (hex.length % 2 !== 0) {
    throw fileError('invalid_response', '服务端返回了非法的十六进制数据')
  }
  const output = new Uint8Array(hex.length / 2)
  for (let index = 0; index < output.length; index += 1) {
    const value = Number.parseInt(hex.slice(index * 2, index * 2 + 2), 16)
    if (Number.isNaN(value)) {
      throw fileError('invalid_response', '服务端返回了非法的十六进制数据')
    }
    output[index] = value
  }
  return output
}

function validateInputName(name: string): string {
  const message = validateName(name)
  if (message) {
    throw fileError('invalid_name', message)
  }
  return name
}

function parseKind(kind: string): NodeKind {
  if (kind === 'file' || kind === 'folder') {
    return kind
  }
  throw fileError('invalid_response', '服务端返回了未知的节点类型')
}

interface NodeKeyEntry {
  wrapped: Uint8Array
}

interface RestoreBody {
  new_parent_id?: string
  name_token?: string
}

export class FileClient implements FileWorkerApi {
  private readonly backend: Promise<CryptoBackend>
  /** node_id → 包裹后的节点密钥（只在 Worker 内存中）。 */
  private readonly wrappedKeys = new Map<string, NodeKeyEntry>()

  constructor(backend: Promise<CryptoBackend>) {
    this.backend = backend
  }

  async listFolder(nodeId: string, cursor?: string): Promise<FolderPageView> {
    const backend = await this.unlockedBackend()
    const query = new URLSearchParams({ limit: '1000' })
    if (cursor) {
      query.set('cursor', cursor)
    }
    const response = await this.request<ChildrenResponse>(
      `/api/v1/nodes/${encodeURIComponent(nodeId)}/children?${query.toString()}`,
      { method: 'GET' },
    )
    const items = response.items.map((summary) =>
      this.decryptNode(backend, summary),
    )
    return {
      items,
      nextCursor: response.next_cursor ?? null,
    }
  }

  async createFolder(parentId: string, name: string): Promise<FileNodeView> {
    validateInputName(name)
    const backend = await this.unlockedBackend()
    const nodeId = crypto.randomUUID()
    const wrapped = backend.createNodeKey(nodeId)
    this.wrappedKeys.set(nodeId, { wrapped })
    const encryptedMeta = backend.encryptNodeMetadata(nodeId, name)
    const nameToken = backend.nameToken(parentId, name)

    const response = await this.request<NodeSummary>('/api/v1/folders', {
      method: 'POST',
      body: JSON.stringify({
        id: nodeId,
        parent_id: parentId,
        wrapped_key: bytesToHex(wrapped),
        encrypted_meta: bytesToHex(encryptedMeta),
        name_token: nameToken,
      }),
    })
    return this.decryptNode(backend, response)
  }

  async renameNode(
    nodeId: string,
    parentId: string,
    name: string,
    rowVersion: number,
  ): Promise<FileNodeView> {
    validateInputName(name)
    const backend = await this.unlockedBackend()
    this.loadCachedNodeKey(backend, nodeId)
    const encryptedMeta = backend.encryptNodeMetadata(nodeId, name)
    const nameToken = backend.nameToken(parentId, name)
    const response = await this.request<NodeSummary>(
      `/api/v1/nodes/${encodeURIComponent(nodeId)}/rename`,
      {
        method: 'PATCH',
        body: JSON.stringify({
          row_version: rowVersion,
          encrypted_meta: bytesToHex(encryptedMeta),
          name_token: nameToken,
        }),
      },
    )
    return this.decryptNode(backend, response)
  }

  async moveNode(
    nodeId: string,
    newParentId: string,
    name: string,
    rowVersion: number,
  ): Promise<FileNodeView> {
    validateInputName(name)
    const backend = await this.unlockedBackend()
    this.loadCachedNodeKey(backend, nodeId)
    const nameToken = backend.nameToken(newParentId, name)
    const response = await this.request<NodeSummary>(
      `/api/v1/nodes/${encodeURIComponent(nodeId)}/move`,
      {
        method: 'PATCH',
        body: JSON.stringify({
          new_parent_id: newParentId,
          row_version: rowVersion,
          name_token: nameToken,
        }),
      },
    )
    return this.decryptNode(backend, response)
  }

  async deleteNode(nodeId: string): Promise<void> {
    await this.request(
      `/api/v1/nodes/${encodeURIComponent(nodeId)}`,
      { method: 'DELETE' },
    )
  }

  async listTrash(): Promise<TrashNodeView[]> {
    const backend = await this.unlockedBackend()
    const response = await this.request<TrashResponse>('/api/v1/trash', {
      method: 'GET',
    })
    const items = response.items.map((summary) => {
      const node = this.decryptNode(backend, summary)
      const deletedAt = summary.deleted_at
      const trashBatchId = summary.trash_batch_id
      if (!deletedAt || !trashBatchId) {
        throw fileError('invalid_response', '回收站项目缺少删除信息')
      }
      return { ...node, deletedAt, trashBatchId }
    })
    return items
  }

  async restoreNode(
    nodeId: string,
    newParentId?: string,
    name?: string,
  ): Promise<FileNodeView> {
    const backend = await this.unlockedBackend()
    const body: RestoreBody = {}
    if (newParentId) {
      if (name === undefined) {
        throw fileError('invalid_name', '选择新位置时必须提供名称')
      }
      validateInputName(name)
      this.loadCachedNodeKey(backend, nodeId)
      body.new_parent_id = newParentId
      body.name_token = backend.nameToken(newParentId, name)
    }
    const response = await this.request<NodeSummary>(
      `/api/v1/trash/${encodeURIComponent(nodeId)}/restore`,
      {
        method: 'POST',
        body: JSON.stringify(body),
      },
    )
    return this.decryptNode(backend, response)
  }

  async permanentlyDeleteNode(nodeId: string): Promise<DeleteResultView> {
    const response = await this.request<PermanentDeleteResponse>(
      `/api/v1/trash/${encodeURIComponent(nodeId)}`,
      { method: 'DELETE' },
    )
    return {
      deletedNodes: response.deleted_nodes,
      deletedVersions: response.deleted_versions,
      deletedContents: response.deleted_contents,
      freedBytes: response.freed_bytes,
      queuedObjects: response.queued_objects,
    }
  }

  async listVersions(nodeId: string): Promise<VersionView[]> {
    const backend = await this.unlockedBackend()
    this.loadCachedNodeKey(backend, nodeId)
    const response = await this.request<VersionsResponse>(
      `/api/v1/nodes/${encodeURIComponent(nodeId)}/versions`,
      { method: 'GET' },
    )
    return response.items.map((version) =>
      this.decryptVersion(backend, nodeId, version),
    )
  }

  async restoreVersion(
    nodeId: string,
    sourceVersionId: string,
  ): Promise<VersionView> {
    const backend = await this.unlockedBackend()
    this.loadCachedNodeKey(backend, nodeId)
    const versions = await this.request<VersionsResponse>(
      `/api/v1/nodes/${encodeURIComponent(nodeId)}/versions`,
      { method: 'GET' },
    )
    const source = versions.items.find((version) => version.id === sourceVersionId)
    if (!source) {
      throw fileError('not_found', '版本不存在')
    }
    const metadata = this.decryptVersion(backend, nodeId, source)
    const newVersionId = crypto.randomUUID()
    const encryptedMeta = backend.encryptVersionMetadata(
      nodeId,
      newVersionId,
      metadata.contentId,
      metadata.size,
      metadata.mtime,
      metadata.mime,
    )
    const response = await this.request<VersionSummary>(
      `/api/v1/nodes/${encodeURIComponent(nodeId)}/versions/restore`,
      {
        method: 'POST',
        body: JSON.stringify({
          source_version_id: sourceVersionId,
          new_version_id: newVersionId,
          encrypted_meta: bytesToHex(encryptedMeta),
        }),
      },
    )
    return this.decryptVersion(backend, nodeId, response)
  }

  async getUsage(): Promise<UsageView> {
    const response = await this.request<UsageResponse>('/api/v1/me/usage', {
      method: 'GET',
    })
    return {
      usedBytes: response.used_bytes,
      reservedBytes: response.reserved_bytes,
      quotaBytes: response.quota_bytes,
    }
  }

  /**
   * Worker 内部使用：返回已缓存的包裹节点密钥（不经过 Comlink）。
   * 只供传输管线在新版本上传/续传时解包节点密钥。
   */
  wrappedKeyFor(nodeId: string): Uint8Array | undefined {
    const cached = this.wrappedKeys.get(nodeId)
    return cached ? cached.wrapped.slice() : undefined
  }

  /** Worker 内部使用：缓存新生成或从会话恢复的包裹节点密钥。 */
  rememberWrappedKey(nodeId: string, wrapped: Uint8Array): void {
    this.wrappedKeys.set(nodeId, { wrapped: wrapped.slice() })
  }

  private async unlockedBackend(): Promise<CryptoBackend> {
    const backend = await this.backend
    if (backend.isLocked()) {
      throw fileError('locked', '密钥库已锁定，请先解锁')
    }
    return backend
  }

  private loadCachedNodeKey(backend: CryptoBackend, nodeId: string): void {
    const cached = this.wrappedKeys.get(nodeId)
    if (!cached) {
      throw fileError('node_key_missing', '节点密钥尚未加载，请刷新目录后重试')
    }
    backend.loadNodeKey(nodeId, cached.wrapped)
  }

  private decryptNode(
    backend: CryptoBackend,
    summary: NodeSummary,
  ): FileNodeView {
    const kind = parseKind(summary.kind)
    const wrappedHex = summary.wrapped_key
    const metaHex = summary.encrypted_meta
    // 根节点没有包裹密钥/元数据；文件浏览器只展示根的子项，不展示根自身。
    if (!wrappedHex || !metaHex) {
      throw fileError('invalid_response', '节点缺少密钥或元数据')
    }
    const wrapped = hexToBytes(wrappedHex)
    this.wrappedKeys.set(summary.id, { wrapped })
    backend.loadNodeKey(summary.id, wrapped)
    const name = backend.decryptNodeMetadata(summary.id, hexToBytes(metaHex))

    let size: number | null = null
    let mtime: number | null = null
    let mime: string | null = null
    const currentVersion = summary.current_version
    if (currentVersion) {
      const metadata = this.decryptVersion(backend, summary.id, currentVersion)
      size = metadata.size
      mtime = metadata.mtime
      mime = metadata.mime
    }

    return {
      id: summary.id,
      parentId: summary.parent_id ?? null,
      kind,
      name,
      size,
      mtime,
      mime,
      rowVersion: summary.row_version,
      currentVersionId: currentVersion?.id ?? null,
      createdAt: summary.created_at,
      updatedAt: summary.updated_at,
    }
  }

  private decryptVersion(
    backend: CryptoBackend,
    nodeId: string,
    version: VersionSummary,
  ): VersionView {
    const metadata = backend.decryptVersionMetadataForContent(
      nodeId,
      version.id,
      version.content_id,
      hexToBytes(version.encrypted_meta),
    )
    return {
      id: version.id,
      contentId: version.content_id,
      size: metadata.size,
      mtime: metadata.mtime,
      mime: metadata.mime,
      isCurrent: version.is_current,
      createdAt: version.created_at,
    }
  }

  private async request<T>(
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
      throw fileError('network_error', '网络请求失败，请稍后重试')
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
      throw fileError(
        typeof error?.code === 'string' ? error.code : 'http_error',
        typeof error?.message === 'string' ? error.message : '请求失败',
      )
    }
    return payload as T
  }
}

/** 创建文件管理 API（Worker 内使用）。 */
export function createFileApi(backend: Promise<CryptoBackend>): FileWorkerApi {
  return new FileClient(backend)
}
