/**
 * 真实 Comlink 边界契约测试（task 8.7 审查修复）。
 *
 * 用 MessageChannel 两端分别 `expose()` / `wrap()` 真实的 WorkerApi，
 * 覆盖上传（带进度回调）、下载、文件管理与认证方法。任何参数/返回值
 * 无法结构化克隆（例如未用 Comlink.proxy 包装的函数）都会让测试失败。
 */
import { expose, wrap } from 'comlink'
import { Blob as NodeBlob, File as NodeFile } from 'node:buffer'
import { describe, expect, it, vi } from 'vitest'

import { wrapApi } from './client'
import { WasmCryptoBackend } from './crypto-backend'
import { ensureWasmForTests } from './test-init'
import type {
  FileNodeView,
  TransferProgressView,
  WorkerApi as WorkerApiType,
} from './types'
import { WorkerApi as RealWorkerApi } from './worker-api'

const KDF_PARAMS =
  '{"algorithm":"argon2id","m_cost_kib":65536,"t_cost":3,"p_cost":1}'
const PASSWORD = 'comlink boundary correct horse battery staple'
const SALT = new Uint8Array(16).fill(0x42)
const ROOT_ID = '550e8400-e29b-41d4-a716-44665544000a'
const USER_ID = '550e8400-e29b-41d4-a716-446655440000'
const EMAIL = 'comlink@example.com'

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  })
}

function requestBody(init: RequestInit | undefined): Record<string, unknown> {
  if (typeof init?.body !== 'string') {
    throw new Error('API 请求体应为 JSON 字符串')
  }
  return JSON.parse(init.body) as Record<string, unknown>
}

interface Session {
  id: string
  contentId: string
  nodeId: string
  versionId: string
  chunkCount: number
  declaredSize: number
  wrappedKey: string
  encryptedMeta: string
  versionMeta: string
  confirmed: boolean
  objectKey?: string
  object?: Uint8Array
}

class ContractServer {
  private session: Session | null = null
  private object: Uint8Array | null = null
  failPut = false

  // fetch 签名要求返回 Promise；该假服务端全部同步构造 Response。
  // eslint-disable-next-line @typescript-eslint/require-await
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
    if (url === '/api/v1/auth/me' && method === 'GET') {
      return json({
        user_id: USER_ID,
        email: EMAIL,
        email_verified: true,
        kdf_params: JSON.parse(KDF_PARAMS) as unknown,
        kdf_salt: hex(new Uint8Array(16).fill(0x11)),
        wrapped_mk: '01'.repeat(73),
        root_node_id: ROOT_ID,
        status: 'active',
        status_reason: null,
        is_admin: false,
      })
    }
    if (url === '/api/v1/me/usage' && method === 'GET') {
      return json({ used_bytes: 123, reserved_bytes: 0, quota_bytes: 1024 })
    }
    if (url === '/api/v1/auth/logout' && method === 'POST') {
      return json({ ok: true })
    }
    if (url === '/api/v1/folders' && method === 'POST') {
      const body = requestBody(init)
      return json({
        ...body,
        kind: 'folder',
        row_version: 1,
        current_version: null,
        created_at: '2026-01-01T00:00:00Z',
        updated_at: '2026-01-01T00:00:00Z',
        deleted_at: null,
        trash_batch_id: null,
      })
    }
    if (url === '/api/v1/upload-sessions' && method === 'POST') {
      const body = requestBody(init)
      this.session = {
        id: 'session-1',
        contentId: body.content_id as string,
        nodeId: body.node_id as string,
        versionId: body.version_id as string,
        chunkCount: body.chunk_count as number,
        declaredSize: body.declared_size as number,
        wrappedKey: body.wrapped_key as string,
        encryptedMeta: body.encrypted_meta as string,
        versionMeta: body.version_meta as string,
        confirmed: false,
      }
      return json(this.sessionResponse())
    }
    if (url.startsWith('https://contract.test/put/') && method === 'PUT') {
      if (this.failPut) {
        return new Response('', { status: 400 })
      }
      const body = init?.body
      if (!(body instanceof Uint8Array)) {
        return new Response('', { status: 400 })
      }
      this.object = new Uint8Array(body)
      return new Response('', { status: 200 })
    }
    if (url.startsWith('https://contract.test/download/') && method === 'GET') {
      return new Response((this.object ?? new Uint8Array()) as unknown as BodyInit, {
        status: 200,
      })
    }
    if (url === '/api/v1/upload-sessions/session-1' && method === 'GET') {
      return json(this.sessionResponse())
    }
    if (url === '/api/v1/upload-sessions/session-1/chunks' && method === 'GET') {
      return json({
        session_id: 'session-1',
        chunk_count: this.session?.chunkCount ?? 0,
        confirmed_count: this.session?.confirmed ? 1 : 0,
        items: this.session?.confirmed
          ? [
              {
                idx: 0,
                declared_size: this.session.declaredSize,
                sha256: 'ab'.repeat(32),
                confirmed_at: '2026-01-01T00:00:00Z',
              },
            ]
          : [],
      })
    }
    if (
      url === '/api/v1/upload-sessions/session-1/chunks/presign' &&
      method === 'POST'
    ) {
      return json({
        session_id: 'session-1',
        items: [
          {
            idx: 0,
            ok: true,
            url: 'https://contract.test/put/key',
            method: 'PUT',
            expires_at: '2030-01-01T00:00:00Z',
            expected_size: this.session?.declaredSize,
            code: null,
            message: null,
          },
        ],
      })
    }
    if (
      url === '/api/v1/upload-sessions/session-1/chunks/confirm' &&
      method === 'POST'
    ) {
      const body = requestBody(init)
      const chunks = body.chunks as Array<{ sha256: string }>
      if (this.session) {
        this.session.confirmed = true
      }
      return json({
        session_id: 'session-1',
        items: chunks.map((chunk) => ({
          idx: 0,
          ok: true,
          declared_size: this.session?.declaredSize,
          sha256: chunk.sha256,
          confirmed_at: '2026-01-01T00:00:00Z',
          code: null,
          message: null,
        })),
      })
    }
    if (url === '/api/v1/upload-sessions/session-1/commit' && method === 'POST') {
      return json({
        session_id: 'session-1',
        status: 'committed',
        node_id: this.session?.nodeId,
        version_id: this.session?.versionId,
        content_id: this.session?.contentId,
        ciphertext_size: this.session?.declaredSize,
        chunk_count: 1,
        committed_at: '2026-01-01T00:00:00Z',
      })
    }
    if (url === '/api/v1/upload-sessions/session-1/abort' && method === 'POST') {
      return json({ session_id: 'session-1', status: 'aborted' })
    }
    if (
      url ===
        `/api/v1/nodes/${this.session?.nodeId}/versions/${this.session?.versionId}/download` &&
      method === 'GET'
    ) {
      return json({
        node_id: this.session?.nodeId,
        version_id: this.session?.versionId,
        content_id: this.session?.contentId,
        encrypted_meta: this.session?.versionMeta,
        ciphertext_size: this.session?.declaredSize,
        chunk_count: 1,
        wrapped_key: this.session?.wrappedKey,
        expires_at: '2030-01-01T00:00:00Z',
        chunks: [
          {
            idx: 0,
            ciphertext_size: this.session?.declaredSize,
            sha256: 'ab'.repeat(32),
            method: 'GET',
            url: 'https://contract.test/download/key',
          },
        ],
      })
    }
    return json({ code: 'not_found', message: 'not found' }, 404)
  }

  private sessionResponse() {
    return {
      id: 'session-1',
      content_id: this.session?.contentId,
      kind: 'new_file',
      status: 'open',
      node_id: this.session?.nodeId,
      parent_id: ROOT_ID,
      version_id: this.session?.versionId,
      chunk_count: this.session?.chunkCount,
      declared_size: this.session?.declaredSize,
      confirmed_chunks: this.session?.confirmed ? 1 : 0,
      wrapped_key: this.session?.wrappedKey,
      encrypted_meta: this.session?.encryptedMeta,
      expires_at: '2030-01-01T00:00:00Z',
      created_at: '2026-01-01T00:00:00Z',
    }
  }
}

function unlockedBackend(): WasmCryptoBackend {
  ensureWasmForTests()
  const backend = new WasmCryptoBackend()
  backend.prepareRegistration(PASSWORD, KDF_PARAMS, SALT)
  backend.commitRegistration()
  return backend
}

function makeFile(): File {
  const bytes = new Uint8Array(100)
  bytes.fill(0x5a)
  // jsdom 的 File 在 Node MessagePort 下会被克隆成空对象；使用 Node 原生
  // File（浏览器中对应真实 File）才能验证 Comlink 的真实跨线程序列化。
  return new NodeFile([bytes], 'comlink.bin', {
    lastModified: 1_700_000_000_000,
    type: 'application/octet-stream',
  }) as unknown as File
}

async function withComlink(
  operation: (api: WorkerApiType) => Promise<void>,
): Promise<void> {
  const server = new ContractServer()
  vi.stubGlobal('fetch', server.fetch)
  const backend = unlockedBackend()
  const workerApi = new RealWorkerApi(Promise.resolve(backend))
  const { port1, port2 } = new MessageChannel()
  expose(workerApi, port1)
  const remote = wrap<WorkerApiType>(port2)
  try {
    await operation(wrapApi(remote))
  } finally {
    port1.close()
    port2.close()
    vi.unstubAllGlobals()
  }
}

describe('真实 Comlink 边界', () => {
  it('上传进度回调、下载 Blob、文件管理与认证方法都可跨线程序列化', async () => {
    await withComlink(async (api) => {
      const progress: TransferProgressView[] = []
      const uploaded = await api.uploadFile(ROOT_ID, makeFile(), (event) => {
        progress.push(event)
      })
      expect(uploaded.nodeId).toBeTruthy()
      expect(progress.length).toBeGreaterThan(0)
      expect(progress.at(-1)?.status).toBe('completed')

      const downloadProgress: TransferProgressView[] = []
      // jsdom 的 Blob 在 Node MessagePort 下会退化为普通对象；改用 Node 原生
      // Blob（浏览器中对应真实 Blob）验证返回值可跨线程克隆。
      vi.stubGlobal('Blob', NodeBlob)
      const result = await api.downloadVersion(
        uploaded.nodeId,
        uploaded.versionId,
        'comlink-download',
        'comlink.bin',
        { kind: 'blob' },
        (event) => downloadProgress.push(event),
      )
      expect(result.blob).not.toBeNull()
      expect(downloadProgress.at(-1)?.status).toBe('completed')
      const bytes = new Uint8Array(await result.blob!.arrayBuffer())
      expect(bytes.byteLength).toBe(100)

      const folder: FileNodeView = await api.createFolder(
        ROOT_ID,
        '契约文件夹',
      )
      expect(folder.name).toBe('契约文件夹')
      const usage = await api.getUsage()
      expect(usage.usedBytes).toBe(123)
      const user = await api.refreshSession()
      expect(user?.email).toBe(EMAIL)
      const pending = await api.listPendingUploads()
      expect(Array.isArray(pending)).toBe(true)
    })
  })

  it('退出登录会清理该用户的续传记录', async () => {
    const server = new ContractServer()
    server.failPut = true
    vi.stubGlobal('fetch', server.fetch)
    const backend = unlockedBackend()
    const api = new RealWorkerApi(Promise.resolve(backend))
    try {
      await api.refreshSession()
      await expect(api.uploadFile(ROOT_ID, makeFile())).rejects.toBeTruthy()
      expect(await api.listPendingUploads()).toHaveLength(1)
      await api.logout()
      expect(await api.listPendingUploads()).toEqual([])
    } finally {
      vi.unstubAllGlobals()
    }
  })
})
