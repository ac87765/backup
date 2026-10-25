/**
 * FileClient 契约测试（Vitest + 真实 WASM，不经过 Comlink）。
 *
 * 重点：Worker 内的解包/解密/令牌计算正确；跨 Comlink 返回的视图不含密钥字节。
 */
import { afterEach, describe, expect, it, vi } from 'vitest'

import { FileClient } from './file-client'
import { WasmCryptoBackend } from './crypto-backend'
import { ensureWasmForTests } from './test-init'

const KDF_PARAMS =
  '{"algorithm":"argon2id","m_cost_kib":65536,"t_cost":3,"p_cost":1}'
const PASSWORD = 'correct horse battery staple'
const SALT = new Uint8Array(16).fill(0x42)

const PARENT_ID = '550e8400-e29b-41d4-a716-44665544000a'
const NODE_ID = '550e8400-e29b-41d4-a716-446655440000'
const VERSION_ID = '6ba7b810-9dad-41d1-80b4-00c04fd430c8'
const CONTENT_ID = '6ba7b811-9dad-41d1-80b4-00c04fd430c8'

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')
}

function response(body: unknown, status = 200) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  })
}

function requestBody(init: RequestInit | undefined): Record<string, string> {
  const raw = init?.body
  if (typeof raw !== 'string') {
    throw new Error('测试请求体必须是 JSON 字符串')
  }
  return JSON.parse(raw) as Record<string, string>
}

function unlockedBackend(): WasmCryptoBackend {
  ensureWasmForTests()
  const backend = new WasmCryptoBackend()
  backend.prepareRegistration(PASSWORD, KDF_PARAMS, SALT)
  backend.commitRegistration()
  return backend
}

function assertNoKeys(value: unknown): void {
  if (value instanceof Uint8Array) {
    expect(value.byteLength).not.toBe(32)
    return
  }
  if (typeof value === 'string') {
    expect(value).not.toMatch(/^[0-9a-fA-F]{64}$/)
    return
  }
  if (Array.isArray(value)) {
    value.forEach(assertNoKeys)
    return
  }
  if (typeof value === 'object' && value !== null) {
    for (const nested of Object.values(value)) {
      assertNoKeys(nested)
    }
  }
}

function nodeSummary(
  backend: WasmCryptoBackend,
  name: string,
  kind: 'file' | 'folder',
) {
  const wrapped = backend.createNodeKey(NODE_ID)
  const encryptedMeta = backend.encryptNodeMetadata(NODE_ID, name)
  return {
    id: NODE_ID,
    kind,
    parent_id: PARENT_ID,
    wrapped_key: hex(wrapped),
    encrypted_meta: hex(encryptedMeta),
    name_token: 'ab'.repeat(32),
    row_version: 1,
    current_version: null,
    created_at: '2026-01-01T00:00:00Z',
    updated_at: '2026-01-01T00:00:00Z',
    deleted_at: null,
    trash_batch_id: null,
  }
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('FileClient', () => {
  it('列目录时在 Worker 内解包密钥并解密名称', async () => {
    const backend = unlockedBackend()
    const client = new FileClient(Promise.resolve(backend))
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        response({
          items: [nodeSummary(backend, '照片', 'folder')],
          next_cursor: null,
        }),
      ),
    )

    const page = await client.listFolder(PARENT_ID)
    expect(page.items).toHaveLength(1)
    expect(page.items[0].name).toBe('照片')
    expect(page.items[0].kind).toBe('folder')
    assertNoKeys(page)
  })

  it('创建文件夹时生成 ID/密钥/元数据/令牌，返回数据不含密钥', async () => {
    const backend = unlockedBackend()
    const client = new FileClient(Promise.resolve(backend))
    const fetchMock = vi.fn((input: unknown, init?: RequestInit) => {
      expect(String(input)).toBe('/api/v1/folders')
      const body = requestBody(init)
      return response({
        ...body,
        kind: 'folder',
        row_version: 1,
        current_version: null,
        created_at: '2026-01-01T00:00:00Z',
        updated_at: '2026-01-01T00:00:00Z',
        deleted_at: null,
        trash_batch_id: null,
      })
    })
    vi.stubGlobal('fetch', fetchMock)

    const node = await client.createFolder(PARENT_ID, '新文件夹')
    expect(node.name).toBe('新文件夹')
    expect(node.id).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
    )
    const body = requestBody(fetchMock.mock.calls[0]?.[1])
    expect(body.parent_id).toBe(PARENT_ID)
    expect(body.wrapped_key).toMatch(/^[0-9a-f]+$/)
    expect(body.encrypted_meta).toMatch(/^[0-9a-f]+$/)
    expect(body.name_token).toMatch(/^[0-9a-f]{64}$/)
    expect(body.wrapped_key).not.toContain('新文件夹')
    assertNoKeys(node)
  })

  it('拒绝非法名称且不发送请求', async () => {
    const backend = unlockedBackend()
    const client = new FileClient(Promise.resolve(backend))
    const fetchMock = vi.fn()
    vi.stubGlobal('fetch', fetchMock)

    await expect(client.createFolder(PARENT_ID, '   ')).rejects.toMatchObject({
      code: 'invalid_name',
    })
    await expect(client.createFolder(PARENT_ID, '../x')).rejects.toMatchObject({
      code: 'invalid_name',
    })
    expect(fetchMock).not.toHaveBeenCalled()
  })

  it('列出版本并复用原 content 恢复旧版本', async () => {
    const backend = unlockedBackend()
    const client = new FileClient(Promise.resolve(backend))
    // 先通过列目录加载/缓存节点密钥。
    const summary = nodeSummary(backend, 'file.txt', 'file')
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(response({ items: [summary], next_cursor: null })),
    )
    await client.listFolder(PARENT_ID)

    const encryptedMeta = backend.encryptVersionMetadata(
      NODE_ID,
      VERSION_ID,
      CONTENT_ID,
      1234,
      1_700_000_000,
      'text/plain',
    )
    const version = {
      id: VERSION_ID,
      content_id: CONTENT_ID,
      encrypted_meta: hex(encryptedMeta),
      ciphertext_size: 1275,
      chunk_count: 1,
      is_current: true,
      created_at: '2026-01-01T00:00:00Z',
    }
    const fetchMock = vi.fn((input: unknown, init?: RequestInit) => {
      const url = String(input)
      if (url.endsWith('/versions')) {
        return response({ items: [version] })
      }
      const body = requestBody(init)
      return response({
        id: body.new_version_id,
        content_id: CONTENT_ID,
        encrypted_meta: body.encrypted_meta,
        ciphertext_size: 1275,
        chunk_count: 1,
        is_current: true,
        created_at: '2026-01-02T00:00:00Z',
      })
    })
    vi.stubGlobal('fetch', fetchMock)

    const versions = await client.listVersions(NODE_ID)
    expect(versions[0].size).toBe(1234)
    expect(versions[0].mime).toBe('text/plain')

    const restored = await client.restoreVersion(NODE_ID, VERSION_ID)
    expect(restored.contentId).toBe(CONTENT_ID)
    expect(restored.size).toBe(1234)
    expect(restored.id).not.toBe(VERSION_ID)

    const restoreCall = fetchMock.mock.calls.find(([url]) =>
      String(url).endsWith('/versions/restore'),
    )
    expect(restoreCall).toBeTruthy()
    const body = requestBody(restoreCall?.[1])
    expect(body.source_version_id).toBe(VERSION_ID)
    expect(body.new_version_id).not.toBe(VERSION_ID)
    expect(body.encrypted_meta).toMatch(/^[0-9a-f]+$/)
    assertNoKeys(restored)
  })
})
