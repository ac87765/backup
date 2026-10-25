/**
 * 文件夹 zip 流式下载（design D7）。
 *
 * 用真实 WASM 后端构造加密元数据/块，FileClient 与对象存储用最小假实现，
 * 验证：目录结构与空文件夹、解压内容一致、任一文件密文被篡改时整体中止。
 */
import { describe, expect, it, vi } from 'vitest'

import { FileClient } from './file-client'
import { TransferClient } from './transfer-client'
import { WasmCryptoBackend } from './crypto-backend'
import { ensureWasmForTests } from './test-init'
import type {
  CryptoBackend,
  FileNodeView,
  FolderPageView,
} from './types'

const KDF_PARAMS =
  '{"algorithm":"argon2id","m_cost_kib":65536,"t_cost":3,"p_cost":1}'
const PASSWORD = 'folder zip correct horse battery staple'
const SALT = new Uint8Array(16).fill(0x42)
const ROOT = '550e8400-e29b-41d4-a716-44665544000a'

function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('')
}

function unlockedBackend(): WasmCryptoBackend {
  ensureWasmForTests()
  const backend = new WasmCryptoBackend()
  backend.prepareRegistration(PASSWORD, KDF_PARAMS, SALT)
  backend.commitRegistration()
  return backend
}

interface EncryptedFile {
  node: FileNodeView
  wrapped: Uint8Array
  versionMeta: Uint8Array
  contentId: string
  chunks: Uint8Array[]
  plaintext: Uint8Array
}

function makeFileNode(
  backend: CryptoBackend,
  options: {
    id: string
    parentId: string
    name: string
    bytes: Uint8Array
    mtime?: number
    mime?: string
  },
): EncryptedFile {
  const versionId = crypto.randomUUID()
  const contentId = crypto.randomUUID()
  const wrapped = backend.createNodeKey(options.id)
  const chunk = backend.encryptChunk(
    options.id,
    contentId,
    0,
    1,
    true,
    options.bytes,
  )
  const versionMeta = backend.encryptVersionMetadata(
    options.id,
    versionId,
    contentId,
    options.bytes.byteLength,
    options.mtime ?? 1_700_000_000,
    options.mime ?? 'application/octet-stream',
    null,
  )
  const node: FileNodeView = {
    id: options.id,
    parentId: options.parentId,
    kind: 'file',
    name: options.name,
    size: options.bytes.byteLength,
    mtime: options.mtime ?? 1_700_000_000,
    mime: options.mime ?? 'application/octet-stream',
    hasContentSha256: false,
    rowVersion: 1,
    currentVersionId: versionId,
    createdAt: '2026-01-01T00:00:00Z',
    updatedAt: '2026-01-01T00:00:00Z',
  }
  return {
    node,
    wrapped,
    versionMeta,
    contentId,
    chunks: [chunk],
    plaintext: options.bytes,
  }
}

function folderNode(
  id: string,
  parentId: string,
  name: string,
): FileNodeView {
  return {
    id,
    parentId,
    kind: 'folder',
    name,
    size: null,
    mtime: null,
    mime: null,
    hasContentSha256: false,
    rowVersion: 1,
    currentVersionId: null,
    createdAt: '2026-01-01T00:00:00Z',
    updatedAt: '2026-01-01T00:00:00Z',
  }
}

function setup() {
  const backend = unlockedBackend()
  const files: EncryptedFile[] = []
  const folders: FileNodeView[] = []
  const tree = new Map<string, FileNodeView[]>()

  const addFile = (options: Parameters<typeof makeFileNode>[1]) => {
    const file = makeFileNode(backend, options)
    files.push(file)
    const list = tree.get(options.parentId) ?? []
    list.push(file.node)
    tree.set(options.parentId, list)
    return file
  }
  const addFolder = (node: FileNodeView) => {
    folders.push(node)
    const list = tree.get(node.parentId ?? '') ?? []
    list.push(node)
    tree.set(node.parentId ?? '', list)
  }

  const fileClient = {
    listFolder: (folderId: string): Promise<FolderPageView> =>
      Promise.resolve({ items: tree.get(folderId) ?? [], nextCursor: null }),
    wrappedKeyFor: (nodeId: string) =>
      files.find((file) => file.node.id === nodeId)?.wrapped,
    rememberWrappedKey: () => undefined,
  } as unknown as FileClient

  let tamper = false
  const fetchImpl = vi.fn((input: RequestInfo | URL) => {
    const url =
      typeof input === 'string'
        ? input
        : input instanceof URL
          ? input.href
          : input.url
    if (url.startsWith('https://storage.test/chunk/')) {
      const contentId = url.split('/').pop() ?? ''
      const file = files.find((candidate) => candidate.contentId === contentId)
      const body = file?.chunks[0] ?? new Uint8Array()
      if (tamper) {
        const copy = body.slice()
        copy[copy.length - 1] = (copy[copy.length - 1] ?? 0) ^ 0xff
        return new Response(copy, { status: 200 })
      }
      return new Response(body as unknown as BodyInit, { status: 200 })
    }
    const download = /^\/api\/v1\/nodes\/([^/]+)\/versions\/([^/]+)\/download$/.exec(
      url,
    )
    if (download) {
      const file = files.find((candidate) => candidate.node.id === download[1])
      if (!file) {
        return new Response(JSON.stringify({ code: 'not_found' }), {
          status: 404,
        })
      }
      return new Response(
        JSON.stringify({
          node_id: file.node.id,
          version_id: file.node.currentVersionId,
          content_id: file.contentId,
          encrypted_meta: hex(file.versionMeta),
          ciphertext_size: file.chunks[0]?.byteLength ?? 0,
          chunk_count: 1,
          wrapped_key: hex(file.wrapped),
          expires_at: '2030-01-01T00:00:00Z',
          chunks: [
            {
              idx: 0,
              ciphertext_size: file.chunks[0]?.byteLength ?? 0,
              sha256: '0'.repeat(64),
              method: 'GET',
              url: `https://storage.test/chunk/${file.contentId}`,
            },
          ],
        }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      )
    }
    return new Response(JSON.stringify({ code: 'not_found' }), { status: 404 })
  })

  const client = new TransferClient(Promise.resolve(backend), fileClient, {
    fetchImpl: fetchImpl as unknown as typeof fetch,
  })
  return {
    client,
    addFile,
    addFolder,
    setTamper: (value: boolean) => {
      tamper = value
    },
  }
}

describe('downloadFolder', () => {
  it('把子树与空文件夹打包为 zip，内容为明文且路径保留', async () => {
    const { client, addFile, addFolder } = setup()
    const photo = folderNode('folder-photo', ROOT, '照片')
    addFolder(photo)
    const sub = folderNode('folder-2025', photo.id, '2025')
    addFolder(sub)
    const empty = folderNode('folder-empty', photo.id, '草稿')
    addFolder(empty)
    const a = addFile({
      id: '550e8400-e29b-41d4-a716-4466554400a1',
      parentId: sub.id,
      name: 'a.txt',
      bytes: new TextEncoder().encode('HELLO-A'),
    })
    const b = addFile({
      id: '550e8400-e29b-41d4-a716-4466554400a2',
      parentId: photo.id,
      name: 'b.txt',
      bytes: new TextEncoder().encode('HELLO-B'),
    })

    const progress: number[] = []
    const result = await client.downloadFolder(
      photo.id,
      '照片.zip',
      'download-folder',
      { kind: 'blob' },
      (event) => progress.push(event.completedChunks),
    )
    expect(result.blob).not.toBeNull()
    const bytes = new Uint8Array(await result.blob!.arrayBuffer())
    const text = new TextDecoder().decode(bytes)
    // STORE：zip 中直接出现明文与条目名。
    expect(text).toContain('HELLO-A')
    expect(text).toContain('HELLO-B')
    expect(text).toContain('2025/a.txt')
    expect(text).toContain('b.txt')
    expect(text).toContain('草稿/')
    expect(bytes[0]).toBe(0x50)
    expect(bytes[1]).toBe(0x4b)
    expect(progress.at(-1)).toBe(2)
    // 加密块本身的哈希字段不应作为明文泄漏到 zip。
    expect(text).not.toContain(hex(a.chunks[0].slice(0, 8)))
    expect(text).not.toContain(hex(b.chunks[0].slice(0, 8)))
  })

  it('任一块密文被篡改时整体中止，不返回完成结果', async () => {
    const { client, addFile, addFolder, setTamper } = setup()
    const folder = folderNode('folder-tamper', ROOT, '损坏')
    addFolder(folder)
    addFile({
      id: '550e8400-e29b-41d4-a716-4466554400a3',
      parentId: folder.id,
      name: 'a.txt',
      bytes: new TextEncoder().encode('SHOULD-NOT-SURVIVE'),
    })
    setTamper(true)

    await expect(
      client.downloadFolder(folder.id, '损坏.zip', 'download-tamper', {
        kind: 'blob',
      }),
    ).rejects.toMatchObject({ code: 'integrity_error' })
  })
})
