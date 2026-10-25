/**
 * 上传会话的 IndexedDB 持久化（task 8.7）。
 *
 * **只保存会话元数据与文件指纹**：会话/内容/节点/版本 ID、父目录、文件名、
 * 大小、lastModified、MIME、状态。绝不保存节点密钥、明文、密文或任何密钥材料。
 *
 * 没有 IndexedDB 的环境（jsdom 单元测试）退化为内存实现；生产 Worker 使用
 * IndexedDB，页面刷新后仍能读取待续传记录。
 */

export type UploadMode = 'new_file' | 'new_version'

export type UploadRecordStatus = 'uploading' | 'paused' | 'failed'

/** 仅元数据；字段与保存的内容不可包含密钥/明文/密文。 */
export interface UploadRecord {
  sessionId: string
  /** 记录所属用户；用于退出登录时只清理该用户的续传记录。 */
  userId: string | null
  mode: UploadMode
  parentId: string | null
  nodeId: string
  contentId: string
  versionId: string
  name: string
  size: number
  lastModified: number
  mime: string
  mtime: number
  status: UploadRecordStatus
  createdAt: number
}

export interface UploadFingerprint {
  name: string
  size: number
  lastModified: number
}

export interface UploadStore {
  put(record: UploadRecord): Promise<void>
  get(sessionId: string): Promise<UploadRecord | null>
  list(): Promise<UploadRecord[]>
  delete(sessionId: string): Promise<void>
  findMatching(
    mode: UploadMode,
    parentId: string | null,
    fingerprint: UploadFingerprint,
    nodeId?: string,
  ): Promise<UploadRecord | null>
}

const DATABASE_NAME = 'backup-uploads'
const STORE_NAME = 'upload-sessions'
const DATABASE_VERSION = 1

function matches(
  record: UploadRecord,
  mode: UploadMode,
  parentId: string | null,
  fingerprint: UploadFingerprint,
  nodeId?: string,
): boolean {
  return (
    record.mode === mode &&
    record.parentId === parentId &&
    (nodeId === undefined || record.nodeId === nodeId) &&
    record.name === fingerprint.name &&
    record.size === fingerprint.size &&
    record.lastModified === fingerprint.lastModified
  )
}

/** 内存实现（测试/无 IndexedDB 环境）。 */
export class MemoryUploadStore implements UploadStore {
  private readonly records = new Map<string, UploadRecord>()

  put(record: UploadRecord): Promise<void> {
    this.records.set(record.sessionId, { ...record })
    return Promise.resolve()
  }

  get(sessionId: string): Promise<UploadRecord | null> {
    const record = this.records.get(sessionId)
    return Promise.resolve(record ? { ...record } : null)
  }

  list(): Promise<UploadRecord[]> {
    return Promise.resolve(
      [...this.records.values()].map((record) => ({ ...record })),
    )
  }

  delete(sessionId: string): Promise<void> {
    this.records.delete(sessionId)
    return Promise.resolve()
  }

  findMatching(
    mode: UploadMode,
    parentId: string | null,
    fingerprint: UploadFingerprint,
    nodeId?: string,
  ): Promise<UploadRecord | null> {
    for (const record of this.records.values()) {
      if (matches(record, mode, parentId, fingerprint, nodeId)) {
        return Promise.resolve({ ...record })
      }
    }
    return Promise.resolve(null)
  }
}

/** 生产实现：IndexedDB 单 object store。 */
export class IndexedDbUploadStore implements UploadStore {
  private readonly database: Promise<IDBDatabase>

  constructor(factory: IDBFactory = indexedDB) {
    this.database = new Promise((resolve, reject) => {
      const request = factory.open(DATABASE_NAME, DATABASE_VERSION)
      request.onupgradeneeded = () => {
        const database = request.result
        if (!database.objectStoreNames.contains(STORE_NAME)) {
          database.createObjectStore(STORE_NAME, { keyPath: 'sessionId' })
        }
      }
      request.onsuccess = () => resolve(request.result)
      request.onerror = () => reject(request.error ?? new Error('IndexedDB 打开失败'))
    })
  }

  private async transaction<T>(
    mode: IDBTransactionMode,
    operation: (store: IDBObjectStore) => IDBRequest<T>,
  ): Promise<T> {
    const database = await this.database
    return new Promise<T>((resolve, reject) => {
      const transaction = database.transaction(STORE_NAME, mode)
      const request = operation(transaction.objectStore(STORE_NAME))
      request.onsuccess = () => resolve(request.result)
      request.onerror = () => reject(request.error ?? new Error('IndexedDB 操作失败'))
    })
  }

  async put(record: UploadRecord): Promise<void> {
    await this.transaction('readwrite', (store) => store.put(record))
  }

  async get(sessionId: string): Promise<UploadRecord | null> {
    const result = await this.transaction<UploadRecord | undefined>(
      'readonly',
      (store) => store.get(sessionId) as IDBRequest<UploadRecord | undefined>,
    )
    return result ?? null
  }

  async list(): Promise<UploadRecord[]> {
    const result = await this.transaction<UploadRecord[]>(
      'readonly',
      (store) => store.getAll() as IDBRequest<UploadRecord[]>,
    )
    return result
  }

  async delete(sessionId: string): Promise<void> {
    await this.transaction('readwrite', (store) => store.delete(sessionId))
  }

  async findMatching(
    mode: UploadMode,
    parentId: string | null,
    fingerprint: UploadFingerprint,
    nodeId?: string,
  ): Promise<UploadRecord | null> {
    const records = await this.list()
    return (
      records.find((record) =>
        matches(record, mode, parentId, fingerprint, nodeId),
      ) ?? null
    )
  }
}

/** 生产选择 IndexedDB；测试环境没有 indexedDB 时退化为内存。 */
export function createUploadStore(): UploadStore {
  if (typeof indexedDB === 'undefined') {
    return new MemoryUploadStore()
  }
  return new IndexedDbUploadStore()
}
