/**
 * Worker 侧的高层加密 API。
 *
 * 这一层不 import WASM：它只依赖注入的 `CryptoBackend`（实现见 `crypto-backend.ts`），
 * 因此可以在 Vitest 里直接对每个方法做契约测试（返回值不得包含密钥字节）。
 *
 * 后端可以是 Promise：Worker 入口在 `expose()` 注册消息监听的同时启动 WASM 初始化，
 * 这样 Worker 启动期间的 RPC 不会丢失（每个方法都会等待后端就绪）。
 *
 * 错误以 `{ code, message }` 普通对象抛出：Comlink 只保证结构化克隆可以传输的数据，
 * 普通对象能完整保留稳定错误码，主线程的 `client.ts` 会把它还原成 `CryptoApiError`。
 */
import type {
  CryptoBackend,
  CryptoWorkerApi,
  VersionMetadataView,
  WrappedMasterKey,
} from './types'

/** 跨 Comlink 传输的错误载荷。 */
export interface CryptoErrorPayload {
  code: string
  message: string
  retryAfter?: number
}

/** 判断一个值是否为 Worker 抛出的错误载荷。 */
export function isCryptoErrorPayload(value: unknown): value is CryptoErrorPayload {
  if (typeof value !== 'object' || value === null) {
    return false
  }
  const candidate = value as Partial<CryptoErrorPayload>
  // 只接受普通对象：Error 实例上的 code 不能跨 Comlink 保留，必须先转成普通对象。
  return (
    !(value instanceof Error) &&
    typeof candidate.code === 'string' &&
    typeof candidate.message === 'string'
  )
}

/** 主线程可读的错误类型（Worker 错误载荷的还原结果）。 */
export class CryptoApiError extends Error {
  /** 稳定错误码，例如 `locked`、`invalid_credentials`。 */
  readonly code: string
  /** 429 响应携带的重试等待秒数。 */
  readonly retryAfter?: number

  constructor(code: string, message: string, retryAfter?: number) {
    super(message)
    this.code = code
    this.retryAfter = retryAfter
    this.name = 'CryptoApiError'
  }
}

function toErrorPayload(error: unknown): CryptoErrorPayload {
  if (isCryptoErrorPayload(error)) {
    return error
  }
  const candidate = error as {
    code?: unknown
    message?: unknown
    retryAfter?: unknown
  } | null
  return {
    code:
      candidate && typeof candidate.code === 'string'
        ? candidate.code
        : 'crypto_error',
    message:
      candidate && typeof candidate.message === 'string'
        ? candidate.message
        : '加密操作失败',
    ...(candidate && typeof candidate.retryAfter === 'number'
      ? { retryAfter: candidate.retryAfter }
      : {}),
  }
}

/** 把后端调用包成 Promise，并把底层错误规范化成可跨 Comlink 的载荷。 */
export class CryptoApi implements CryptoWorkerApi {
  private readonly backend: Promise<CryptoBackend>

  constructor(backend: CryptoBackend | Promise<CryptoBackend>) {
    this.backend = Promise.resolve(backend)
  }

  private async run<T>(operation: (backend: CryptoBackend) => T): Promise<T> {
    try {
      const backend = await this.backend
      return operation(backend)
    } catch (error) {
      // Comlink 只可靠传输结构化克隆的数据；Error 上的自定义 code 会丢失，
      // 因此这里刻意抛出普通对象，由主线程的 client.ts 还原成 CryptoApiError。
      // eslint-disable-next-line @typescript-eslint/only-throw-error
      throw toErrorPayload(error)
    }
  }

  createMasterKey(
    password: string,
    kdfParamsJson: string,
    salt: Uint8Array,
  ): Promise<WrappedMasterKey> {
    return this.run((backend) => {
      backend.derivePasswordKeys(password, kdfParamsJson, salt)
      return { wrappedMk: backend.createMasterKey() }
    })
  }

  unlock(
    password: string,
    kdfParamsJson: string,
    salt: Uint8Array,
    wrappedMk: Uint8Array,
  ): Promise<void> {
    return this.run((backend) => {
      backend.derivePasswordKeys(password, kdfParamsJson, salt)
      backend.unlockWithPassword(wrappedMk)
    })
  }

  lock(): Promise<void> {
    return this.run((backend) => {
      backend.lock()
    })
  }

  isLocked(): Promise<boolean> {
    return this.run((backend) => backend.isLocked())
  }

  createNodeKey(nodeId: string): Promise<Uint8Array> {
    return this.run((backend) => backend.createNodeKey(nodeId))
  }

  loadNodeKey(nodeId: string, wrapped: Uint8Array): Promise<void> {
    return this.run((backend) => {
      backend.loadNodeKey(nodeId, wrapped)
    })
  }

  encryptNodeMetadata(nodeId: string, name: string): Promise<Uint8Array> {
    return this.run((backend) => backend.encryptNodeMetadata(nodeId, name))
  }

  decryptNodeMetadata(
    nodeId: string,
    ciphertext: Uint8Array,
  ): Promise<string> {
    return this.run((backend) => backend.decryptNodeMetadata(nodeId, ciphertext))
  }

  nameToken(parentId: string, name: string): Promise<string> {
    return this.run((backend) => backend.nameToken(parentId, name))
  }

  encryptVersionMetadata(
    nodeId: string,
    versionId: string,
    contentId: string,
    size: number,
    mtime: number,
    mime: string,
  ): Promise<Uint8Array> {
    return this.run((backend) =>
      backend.encryptVersionMetadata(
        nodeId,
        versionId,
        contentId,
        size,
        mtime,
        mime,
      ),
    )
  }

  decryptVersionMetadataForContent(
    nodeId: string,
    versionId: string,
    contentId: string,
    ciphertext: Uint8Array,
  ): Promise<VersionMetadataView> {
    return this.run((backend) =>
      backend.decryptVersionMetadataForContent(
        nodeId,
        versionId,
        contentId,
        ciphertext,
      ),
    )
  }

  chunkCount(size: number): Promise<number> {
    return this.run((backend) => backend.chunkCount(size))
  }

  encryptChunk(
    nodeId: string,
    contentId: string,
    index: number,
    count: number,
    isLast: boolean,
    plaintext: Uint8Array,
  ): Promise<Uint8Array> {
    return this.run((backend) =>
      backend.encryptChunk(
        nodeId,
        contentId,
        index,
        count,
        isLast,
        plaintext,
      ),
    )
  }

  decryptChunk(
    nodeId: string,
    contentId: string,
    index: number,
    count: number,
    isLast: boolean,
    ciphertext: Uint8Array,
  ): Promise<Uint8Array> {
    return this.run((backend) =>
      backend.decryptChunk(
        nodeId,
        contentId,
        index,
        count,
        isLast,
        ciphertext,
      ),
    )
  }

  verifyPlaintextSize(expected: number, actual: number): Promise<void> {
    return this.run((backend) => {
      backend.verifyPlaintextSize(expected, actual)
    })
  }
}

/** 用给定后端（或后端 Promise）创建 Worker API。 */
export function createCryptoApi(
  backend: CryptoBackend | Promise<CryptoBackend>,
): CryptoApi {
  return new CryptoApi(backend)
}



