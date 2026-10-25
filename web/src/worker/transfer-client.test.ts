/**
 * TransferClient 契约测试（Vitest + 真实 WASM + 故障注入的假服务端/对象存储）。
 *
 * 覆盖：上传重试、403 后重新 presign、刷新后只传缺失块、指纹不符、
 * 并发/内存上限、下载完整性校验。
 */
import { describe, expect, it, vi } from 'vitest'

import { FileClient } from './file-client'
import { TransferClient, CHUNK_FRAME_OVERHEAD, CHUNK_PLAINTEXT_SIZE } from './transfer-client'
import { WasmCryptoBackend } from './crypto-backend'
import { ensureWasmForTests } from './test-init'
import { MemoryUploadStore } from './upload-store'
import type { CryptoBackend, TransferProgressView } from './types'

const KDF_PARAMS =
  '{"algorithm":"argon2id","m_cost_kib":65536,"t_cost":3,"p_cost":1}'
const PASSWORD = 'transfer pipeline correct horse battery staple'
const SALT = new Uint8Array(16).fill(0x42)
const ROOT_ID = '550e8400-e29b-41d4-a716-44665544000a'
const FULL = CHUNK_PLAINTEXT_SIZE + CHUNK_FRAME_OVERHEAD

interface SessionRecord {
  id: string
  kind: 'new_file' | 'new_version'
  status: 'open' | 'committed' | 'aborted'
  content_id: string
  node_id: string
  version_id: string
  chunk_count: number
  declared_size: number
  wrapped_key: string | null
  encrypted_meta: string | null
  version_meta: string
  confirmed: Map<number, { key: string; size: number; sha256: string }>
  current_keys: Map<number, string>
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  })
}

function bodyText(init: RequestInit | undefined): Record<string, unknown> {
  if (typeof init?.body !== 'string') {
    throw new Error('API 请求体应为 JSON 字符串')
  }
  return JSON.parse(init.body) as Record<string, unknown>
}

class FakeServer {
  private readonly sessions = new Map<string, SessionRecord>()
  private readonly objects = new Map<string, Uint8Array>()
  private counter = 0
  failPutFor = new Map<number, { status: number; remaining: number }>()
  expireFirstPut = false
  private expiredKeys = new Set<string>()
  putAttempts = new Map<number, number>()
  presignCalls = 0
  activePuts = 0
  peakPuts = 0
  firstPutSliceCalls: number | null = null
  tamperContentId = false
  tamperCiphertext = false
  putDelayMs = 5
  private readonly sliceCalls: () => number

  constructor(sliceCalls: () => number = () => 0) {
    this.sliceCalls = sliceCalls
  }

  get sessionsCount(): number {
    return this.sessions.size
  }

  session(id: string): SessionRecord {
    const session = this.sessions.get(id)
    if (!session) {
      throw new Error(`unknown session ${id}`)
    }
    return session
  }

  sessionResponse(session: SessionRecord) {
    const confirmed = [...session.confirmed.values()]
    return {
      id: session.id,
      content_id: session.content_id,
      kind: session.kind,
      status: session.status,
      node_id: session.node_id,
      parent_id: session.kind === 'new_file' ? ROOT_ID : null,
      version_id: session.version_id,
      chunk_count: session.chunk_count,
      declared_size: session.declared_size,
      confirmed_chunks: confirmed.length,
      wrapped_key: session.wrapped_key,
      encrypted_meta: session.encrypted_meta,
      expires_at: '2030-01-01T00:00:00Z',
      created_at: '2026-01-01T00:00:00Z',
    }
  }

  private expectedSize(session: SessionRecord, idx: number): number {
    if (idx === session.chunk_count - 1) {
      return session.declared_size - (session.chunk_count - 1) * FULL
    }
    return FULL
  }

  readonly fetch = async (
    input: RequestInfo | URL,
    init?: RequestInit,
  ): Promise<Response> => {
    const url =
      typeof input === 'string'
        ? input
        : input instanceof URL
          ? input.href
          : input.url
    const method = init?.method ?? 'GET'
    if (url.startsWith('https://storage.test/put/')) {
      return this.putObject(url, init)
    }
    if (url.startsWith('https://storage.test/download/')) {
      return this.downloadObject(url)
    }
    if (method === 'POST' && url === '/api/v1/upload-sessions') {
      return this.createSession(bodyText(init))
    }
    const sessionMatch = /^\/api\/v1\/upload-sessions\/([^/]+)(\/.*)?$/.exec(url)
    if (sessionMatch) {
      const session = this.session(sessionMatch[1])
      const suffix = sessionMatch[2] ?? ''
      if (method === 'GET' && suffix === '') {
        return json(this.sessionResponse(session))
      }
      if (method === 'GET' && suffix === '/chunks') {
        return json({
          session_id: session.id,
          chunk_count: session.chunk_count,
          confirmed_count: session.confirmed.size,
          items: [...session.confirmed.entries()]
            .sort(([a], [b]) => a - b)
            .map(([idx, chunk]) => ({
              idx,
              declared_size: chunk.size,
              sha256: chunk.sha256,
              confirmed_at: '2026-01-01T00:00:00Z',
            })),
        })
      }
      if (method === 'POST' && suffix === '/chunks/presign') {
        return this.presign(session, bodyText(init))
      }
      if (method === 'POST' && suffix === '/chunks/confirm') {
        return this.confirm(session, bodyText(init))
      }
      if (method === 'POST' && suffix === '/commit') {
        session.status = 'committed'
        return json({
          session_id: session.id,
          status: 'committed',
          node_id: session.node_id,
          version_id: session.version_id,
          content_id: session.content_id,
          ciphertext_size: session.declared_size,
          chunk_count: session.chunk_count,
          committed_at: '2026-01-01T00:00:00Z',
        })
      }
      if (method === 'POST' && suffix === '/abort') {
        session.status = 'aborted'
        return json({ session_id: session.id, status: 'aborted' })
      }
    }
    const downloadMatch = /^\/api\/v1\/nodes\/([^/]+)\/versions\/([^/]+)\/download$/.exec(
      url,
    )
    if (downloadMatch) {
      return this.downloadManifest(downloadMatch[1], downloadMatch[2])
    }
    return json({ code: 'not_found', message: 'not found' }, 404)
  }

  private createSession(body: Record<string, unknown>): Response {
    const id = `session-${++this.counter}`
    const session: SessionRecord = {
      id,
      kind: body.kind as 'new_file' | 'new_version',
      status: 'open',
      content_id: body.content_id as string,
      node_id: body.node_id as string,
      version_id: body.version_id as string,
      chunk_count: body.chunk_count as number,
      declared_size: body.declared_size as number,
      wrapped_key:
        body.kind === 'new_file' ? (body.wrapped_key as string) : null,
      encrypted_meta:
        body.kind === 'new_file' ? (body.encrypted_meta as string) : null,
      version_meta: body.version_meta as string,
      confirmed: new Map(),
      current_keys: new Map(),
    }
    this.sessions.set(id, session)
    return json(this.sessionResponse(session))
  }

  private presign(session: SessionRecord, body: Record<string, unknown>): Response {
    this.presignCalls += 1
    const indices = body.indices as number[]
    const items = indices.map((idx) => {
      const key = `${session.id}/${idx}/${this.counter++}`
      session.current_keys.set(idx, key)
      if (this.expireFirstPut) {
        this.expireFirstPut = false
        this.expiredKeys.add(key)
      }
      return {
        idx,
        ok: true,
        url: `https://storage.test/put/${key}`,
        method: 'PUT',
        expires_at: '2030-01-01T00:00:00Z',
        expected_size: this.expectedSize(session, idx),
        code: null,
        message: null,
      }
    })
    return json({ session_id: session.id, items })
  }

  private confirm(session: SessionRecord, body: Record<string, unknown>): Response {
    const chunks = body.chunks as Array<{
      idx: number
      declared_size: number
      sha256: string
    }>
    const items = chunks.map((chunk) => {
      const key = session.current_keys.get(chunk.idx)
      const object = key ? this.objects.get(key) : undefined
      if (
        !key ||
        !object ||
        object.byteLength !== this.expectedSize(session, chunk.idx) ||
        object.byteLength !== chunk.declared_size
      ) {
        return {
          idx: chunk.idx,
          ok: false,
          declared_size: null,
          sha256: null,
          confirmed_at: null,
          code: 'chunk_not_found',
          message: '块对象不存在',
        }
      }
      session.confirmed.set(chunk.idx, {
        key,
        size: object.byteLength,
        sha256: chunk.sha256,
      })
      return {
        idx: chunk.idx,
        ok: true,
        declared_size: object.byteLength,
        sha256: chunk.sha256,
        confirmed_at: '2026-01-01T00:00:00Z',
        code: null,
        message: null,
      }
    })
    return json({ session_id: session.id, items })
  }

  private async putObject(
    url: string,
    init: RequestInit | undefined,
  ): Promise<Response> {
    this.activePuts += 1
    this.peakPuts = Math.max(this.peakPuts, this.activePuts)
    try {
      const key = url.replace('https://storage.test/put/', '')
      const idx = Number(key.split('/')[1])
      this.putAttempts.set(idx, (this.putAttempts.get(idx) ?? 0) + 1)
      if (this.firstPutSliceCalls === null) {
        this.firstPutSliceCalls = this.sliceCalls()
      }
      await new Promise((resolve) => setTimeout(resolve, this.putDelayMs))
      const failure = this.failPutFor.get(idx)
      if (failure && failure.remaining > 0) {
        failure.remaining -= 1
        return new Response('', { status: failure.status })
      }
      if (this.expiredKeys.has(key)) {
        return new Response('', { status: 403 })
      }
      const body: BodyInit | null | undefined = init?.body
      if (
        body === null ||
        body === undefined ||
        typeof body === 'string' ||
        !(body instanceof Uint8Array)
      ) {
        return new Response('', { status: 400 })
      }
      this.objects.set(key, new Uint8Array(body))
      return new Response('', { status: 200 })
    } finally {
      this.activePuts -= 1
    }
  }

  private downloadObject(url: string): Response {
    const key = url.replace('https://storage.test/download/', '')
    const object = this.objects.get(key)
    if (!object) {
      return new Response('', { status: 404 })
    }
    const body = this.tamperCiphertext ? tamper(object) : object
    return new Response(body as unknown as BodyInit, { status: 200 })
  }

  private downloadManifest(nodeId: string, versionId: string): Response {
    const session = [...this.sessions.values()].find(
      (candidate) =>
        candidate.node_id === nodeId &&
        candidate.version_id === versionId &&
        candidate.status === 'committed',
    )
    if (!session) {
      return json({ code: 'not_found', message: '版本不存在' }, 404)
    }
    const chunks = [...session.confirmed.entries()]
      .sort(([a], [b]) => a - b)
      .map(([idx, chunk]) => ({
        idx,
        ciphertext_size: chunk.size,
        sha256: chunk.sha256,
        method: 'GET',
        url: `https://storage.test/download/${chunk.key}`,
      }))
    return json({
      node_id: session.node_id,
      version_id: session.version_id,
      content_id: this.tamperContentId
        ? '00000000-0000-4000-8000-000000000000'
        : session.content_id,
      encrypted_meta: session.version_meta,
      ciphertext_size: session.declared_size,
      chunk_count: session.chunk_count,
      wrapped_key: session.wrapped_key,
      expires_at: '2030-01-01T00:00:00Z',
      chunks,
    })
  }
}

function tamper(bytes: Uint8Array): Uint8Array {
  const copy = bytes.slice()
  if (copy.length > 0) {
    copy[copy.length - 1] = (copy[copy.length - 1] ?? 0) ^ 0xff
  }
  return copy
}

function unlockedBackend(): CryptoBackend {
  ensureWasmForTests()
  const backend = new WasmCryptoBackend()
  backend.prepareRegistration(PASSWORD, KDF_PARAMS, SALT)
  backend.commitRegistration()
  return backend
}

function makeFile(size: number, name: string, lastModified = 1_700_000_000_000): File {
  const bytes = new Uint8Array(size)
  for (let index = 0; index < size; index += 1) {
    bytes[index] = index % 251
  }
  return new File([bytes], name, {
    lastModified,
    type: 'application/octet-stream',
  })
}

async function fileBytes(file: File): Promise<Uint8Array> {
  return new Uint8Array(await file.arrayBuffer())
}

function setup(options: { store?: MemoryUploadStore; server?: FakeServer } = {}) {
  const backend = unlockedBackend()
  const fileClient = new FileClient(Promise.resolve(backend))
  const store = options.store ?? new MemoryUploadStore()
  const server = options.server ?? new FakeServer()
  const client = new TransferClient(Promise.resolve(backend), fileClient, {
    store,
    fetchImpl: server.fetch,
  })
  return { backend, fileClient, store, server, client }
}

describe('TransferClient', () => {
  it('上传多块文件并下载，明文与原始文件一致', async () => {
    const { client, server } = setup()
    const file = makeFile(CHUNK_PLAINTEXT_SIZE + 100, 'photo.bin')
    const progress: TransferProgressView[] = []
    const result = await client.uploadFile(ROOT_ID, file, (event) => {
      progress.push(event)
    })
    expect(result.size).toBe(file.size)
    expect(server.putAttempts.get(0)).toBe(1)
    expect(server.putAttempts.get(1)).toBe(1)
    expect(progress.at(-1)?.status).toBe('completed')

    const download = await client.downloadVersion(
      result.nodeId,
      result.versionId,
      'download-1',
      file.name,
      { kind: 'blob' },
    )
    expect(download.blob).not.toBeNull()
    const downloaded = new Uint8Array(await download.blob!.arrayBuffer())
    expect(downloaded).toEqual(await fileBytes(file))
  })

  it('PUT 遇到瞬时 503 会重试，403 会重新 presign', async () => {
    const { client, server } = setup()
    const file = makeFile(100, 'retry.bin')
    server.failPutFor.set(0, { status: 503, remaining: 2 })
    server.expireFirstPut = true
    await client.uploadFile(ROOT_ID, file)
    expect(server.putAttempts.get(0)).toBeGreaterThanOrEqual(3)
    expect(server.presignCalls).toBeGreaterThanOrEqual(2)
  })

  it('失败后只续传缺失块；指纹不符时拒绝续传', async () => {
    const { client, server, store, backend } = setup()
    const file = makeFile(CHUNK_PLAINTEXT_SIZE + 100, 'resume.bin')
    server.failPutFor.set(1, { status: 400, remaining: 1 })
    await expect(client.uploadFile(ROOT_ID, file)).rejects.toMatchObject({
      code: 'http_error',
    })
    const pending = await store.list()
    expect(pending).toHaveLength(1)
    const record = pending[0]
    // IndexedDB 记录只允许元数据：不能出现任何密钥/明文/密文字节。
    expect(Object.keys(record).sort()).toEqual([
      'contentId',
      'createdAt',
      'lastModified',
      'mime',
      'mode',
      'mtime',
      'name',
      'nodeId',
      'parentId',
      'sessionId',
      'size',
      'status',
      'userId',
      'versionId',
    ])
    for (const value of Object.values(record)) {
      expect(value).not.toBeInstanceOf(Uint8Array)
    }
    expect(server.putAttempts.get(0)).toBe(1)
    expect(server.putAttempts.get(1)).toBe(1)

    const different = makeFile(file.size, 'resume.bin', file.lastModified + 1)
    await expect(
      client.resumePendingUpload(record.sessionId, different),
    ).rejects.toMatchObject({ code: 'fingerprint_mismatch' })

    server.failPutFor.delete(1)
    const resumed = await client.resumePendingUpload(record.sessionId, file)
    expect(resumed.nodeId).toBe(record.nodeId)
    expect(server.putAttempts.get(0), '已确认块不应重传').toBe(1)
    expect(server.putAttempts.get(1)).toBe(2)
    expect(await store.list()).toHaveLength(0)
    expect(backend.isLocked()).toBe(false)
  })

  it('并发 PUT 不超过 4，且不会一次性读取全部块', async () => {
    const backend = unlockedBackend()
    const file = makeFile(4 * CHUNK_PLAINTEXT_SIZE + 100, 'large.bin')
    const sliceSpy = vi.spyOn(file, 'slice')
    const server = new FakeServer(() => sliceSpy.mock.calls.length)
    const fileClient = new FileClient(Promise.resolve(backend))
    const client = new TransferClient(Promise.resolve(backend), fileClient, {
      store: new MemoryUploadStore(),
      fetchImpl: server.fetch,
    })
    await client.uploadFile(ROOT_ID, file)
    expect(server.firstPutSliceCalls ?? 0).toBeLessThanOrEqual(4)
    expect(server.peakPuts).toBeLessThanOrEqual(4)
    expect(server.putAttempts.size).toBe(5)
  })

  it('下载时 content_id 不符或密文被篡改都会报完整性错误', async () => {
    const { client, server } = setup()
    const file = makeFile(100, 'integrity.bin')
    const result = await client.uploadFile(ROOT_ID, file)

    server.tamperContentId = true
    await expect(
      client.downloadVersion(
        result.nodeId,
        result.versionId,
        'download-2',
        file.name,
        { kind: 'blob' },
      ),
    ).rejects.toMatchObject({ code: 'integrity_error' })

    server.tamperContentId = false
    server.tamperCiphertext = true
    await expect(
      client.downloadVersion(
        result.nodeId,
        result.versionId,
        'download-3',
        file.name,
        { kind: 'blob' },
      ),
    ).rejects.toMatchObject({ code: 'integrity_error' })
  })

  it('取消上传会中止服务端会话并删除待续传记录', async () => {
    const { client, server, store } = setup()
    server.putDelayMs = 100
    const file = makeFile(CHUNK_PLAINTEXT_SIZE + 100, 'cancel.bin')
    let sessionId = ''
    const upload = client.uploadFile(ROOT_ID, file, (progress) => {
      if (!sessionId) {
        sessionId = progress.transferId
      }
    })
    void upload.catch(() => undefined)
    for (let attempt = 0; attempt < 100 && !sessionId; attempt += 1) {
      await new Promise((resolve) => setTimeout(resolve, 10))
    }
    expect(sessionId).not.toBe('')
    await client.cancelUpload(sessionId)
    await expect(upload).rejects.toMatchObject({ code: 'cancelled' })
    expect(server.session(sessionId).status).toBe('aborted')
    expect(await store.list()).toHaveLength(0)
  })

  it('Service Worker 目标把解密块与结束信号送入 BroadcastChannel', async () => {
    const { client } = setup()
    const file = makeFile(100, 'sw.bin')
    const result = await client.uploadFile(ROOT_ID, file)

    class FakeChannel {
      static instances: FakeChannel[] = []
      readonly name: string
      readonly messages: unknown[] = []
      closed = false

      constructor(name: string) {
        this.name = name
        FakeChannel.instances.push(this)
      }

      postMessage(message: unknown): void {
        this.messages.push(message)
      }

      close(): void {
        this.closed = true
      }
    }
    vi.stubGlobal('BroadcastChannel', FakeChannel)
    try {
      const download = await client.downloadVersion(
        result.nodeId,
        result.versionId,
        'download-sw',
        file.name,
        { kind: 'service-worker', downloadId: 'download-sw' },
      )
      expect(download.blob).toBeNull()
      const channel = FakeChannel.instances[0]
      expect(channel?.name).toBe('download-sw')
      expect(channel?.messages[0]).toMatchObject({ type: 'chunk' })
      expect(channel?.messages.at(-1)).toMatchObject({ type: 'end' })
      expect(channel?.closed).toBe(true)
    } finally {
      vi.unstubAllGlobals()
    }
  })
})
