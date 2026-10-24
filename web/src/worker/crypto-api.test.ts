/**
 * Worker API 契约测试（Vitest，直接调用 `CryptoApi`，不经过 Comlink）。
 *
 * 重点：**每个导出方法的返回值都不得包含密钥字节**。密钥只允许存在于 WASM 内存；
 * 跨线程只允许出现包裹后的密文、明文、名称令牌、布尔值与普通元数据对象。
 */
import { describe, expect, it, vi } from 'vitest'

import { CryptoApi } from './crypto-api'
import { WasmCryptoBackend } from './crypto-backend'
import { ensureWasmForTests } from './test-init'
import type { CryptoWorkerApi } from './types'

const KDF_PARAMS =
  '{"algorithm":"argon2id","m_cost_kib":65536,"t_cost":3,"p_cost":1}'
const PASSWORD = 'correct horse battery staple'
const SALT = new Uint8Array(16).fill(0x42)

const NODE_ID = '550e8400-e29b-41d4-a716-446655440000'
const PARENT_ID = '550e8400-e29b-41d4-a716-44665544000a'
const VERSION_ID = '6ba7b810-9dad-41d1-80b4-00c04fd430c8'
const CONTENT_ID = '6ba7b811-9dad-41d1-80b4-00c04fd430c8'

function makeApi(): CryptoApi {
  ensureWasmForTests()
  return new CryptoApi(new WasmCryptoBackend())
}

async function unlockedApi(): Promise<CryptoApi> {
  const api = makeApi()
  await api.createMasterKey(PASSWORD, KDF_PARAMS, SALT)
  await api.createNodeKey(NODE_ID)
  return api
}

/** 校验一个值是“帧格式密文”：0x01 版本 + 明文长度 + 41 字节开销。 */
function expectCiphertext(value: unknown, plaintextLength: number): void {
  expect(value).toBeInstanceOf(Uint8Array)
  const bytes = value as Uint8Array
  expect(bytes.length).toBe(plaintextLength + 41)
  expect(bytes[0]).toBe(0x01)
}

/** 禁止出现的字段名：任何看起来像密钥材料的内容。 */
const FORBIDDEN_KEY_NAMES =
  /^(mk|masterKey|nodeKey|contentKey|nameKey|pwdWrap|pwdWrapKey|rkWrap|rkAuth|authKey|recoveryAuth|recoveryKey|key|secret)$/i

/**
 * 断言返回值结构中不含密钥字段。
 *
 * `Uint8Array` 只允许作为密文/明文出现——调用方在各自的断言里会验证其语义
 * （帧版本、长度、能否解回输入），因此这里不再按长度猜测。
 */
function assertNoKeyMaterial(value: unknown): void {
  if (
    value instanceof Uint8Array ||
    typeof value === 'string' ||
    typeof value === 'number' ||
    typeof value === 'boolean' ||
    value === undefined ||
    value === null
  ) {
    return
  }
  if (Array.isArray(value)) {
    value.forEach(assertNoKeyMaterial)
    return
  }
  expect(value).toBeTypeOf('object')
  for (const [key, nested] of Object.entries(value as Record<string, unknown>)) {
    expect(key).not.toMatch(FORBIDDEN_KEY_NAMES)
    assertNoKeyMaterial(nested)
  }
}

/**
 * 覆盖 `CryptoWorkerApi` 的每个方法。
 *
 * `Record<keyof CryptoWorkerApi, true>` 让 TypeScript 在新增 API 方法而忘记测试时直接报错。
 */
const METHOD_MANIFEST: Record<keyof CryptoWorkerApi, true> = {
  createMasterKey: true,
  unlock: true,
  lock: true,
  isLocked: true,
  createNodeKey: true,
  loadNodeKey: true,
  encryptNodeMetadata: true,
  decryptNodeMetadata: true,
  nameToken: true,
  encryptVersionMetadata: true,
  decryptVersionMetadataForContent: true,
  chunkCount: true,
  encryptChunk: true,
  decryptChunk: true,
  verifyPlaintextSize: true,
}

describe('Crypto Worker API 契约', () => {
  it('导出的方法与契约清单一一对应', () => {
    const implemented = Object.getOwnPropertyNames(CryptoApi.prototype).filter(
      (name) => name !== 'constructor' && !name.startsWith('_') && name !== 'run',
    )
    expect(new Set(implemented)).toEqual(new Set(Object.keys(METHOD_MANIFEST)))
  })

  it('每个导出方法都只返回密文/明文/令牌/布尔值', async () => {
    const api = makeApi()

    // createMasterKey：返回包裹后的 MK（密文），不是密钥本身。
    const { wrappedMk } = await api.createMasterKey(PASSWORD, KDF_PARAMS, SALT)
    expectCiphertext(wrappedMk, 32)
    assertNoKeyMaterial({ wrappedMk })

    // isLocked：布尔值。
    expect(await api.isLocked()).toBe(false)
    assertNoKeyMaterial(await api.isLocked())

    // lock：无返回值。
    await expect(api.lock()).resolves.toBeUndefined()
    expect(await api.isLocked()).toBe(true)

    // unlock：无返回值。
    await expect(
      api.unlock(PASSWORD, KDF_PARAMS, SALT, wrappedMk),
    ).resolves.toBeUndefined()

    // createNodeKey：返回包裹后的节点密钥（密文）。
    const wrappedNodeKey = await api.createNodeKey(NODE_ID)
    expectCiphertext(wrappedNodeKey, 32)
    assertNoKeyMaterial(wrappedNodeKey)

    // loadNodeKey：无返回值。
    await expect(
      api.loadNodeKey(NODE_ID, wrappedNodeKey),
    ).resolves.toBeUndefined()

    // 节点元数据：密文 + 明文文件名。
    const metadataCiphertext = await api.encryptNodeMetadata(NODE_ID, '报告.pdf')
    expect(metadataCiphertext).toBeInstanceOf(Uint8Array)
    assertNoKeyMaterial(metadataCiphertext)
    expect(await api.decryptNodeMetadata(NODE_ID, metadataCiphertext)).toBe(
      '报告.pdf',
    )

    // 名称令牌：64 位十六进制（服务端可见的令牌，不是密钥）。
    const token = await api.nameToken(PARENT_ID, 'Report.PDF')
    expect(token).toMatch(/^[0-9a-f]{64}$/)
    expect(await api.nameToken(PARENT_ID, 'report.pdf')).toBe(token)
    assertNoKeyMaterial(token)

    // 版本元数据：密文 + 只读视图。
    const versionCiphertext = await api.encryptVersionMetadata(
      NODE_ID,
      VERSION_ID,
      CONTENT_ID,
      1024,
      1_700_000_000,
      'text/plain',
    )
    expect(versionCiphertext).toBeInstanceOf(Uint8Array)
    assertNoKeyMaterial(versionCiphertext)

    const versionView = await api.decryptVersionMetadataForContent(
      NODE_ID,
      VERSION_ID,
      CONTENT_ID,
      versionCiphertext,
    )
    expect(versionView).toEqual({
      contentId: CONTENT_ID,
      size: 1024,
      mtime: 1_700_000_000,
      mime: 'text/plain',
    })
    assertNoKeyMaterial(versionView)

    // chunkCount：数字。
    expect(await api.chunkCount(0)).toBe(1)
    assertNoKeyMaterial(await api.chunkCount(0))

    // 分块：密文 + 明文。
    const plaintext = new TextEncoder().encode('hello chunk')
    const chunkCiphertext = await api.encryptChunk(
      NODE_ID,
      CONTENT_ID,
      0,
      1,
      true,
      plaintext,
    )
    expectCiphertext(chunkCiphertext, plaintext.length)
    assertNoKeyMaterial(chunkCiphertext)

    const decrypted = await api.decryptChunk(
      NODE_ID,
      CONTENT_ID,
      0,
      1,
      true,
      chunkCiphertext,
    )
    expect(Array.from(decrypted)).toEqual(Array.from(plaintext))
    assertNoKeyMaterial(decrypted)

    // verifyPlaintextSize：成功时无返回值；失败时返回稳定错误码。
    await expect(api.verifyPlaintextSize(3, 3)).resolves.toBeUndefined()
    await expect(api.verifyPlaintextSize(3, 2)).rejects.toMatchObject({
      code: 'plaintext_size_mismatch',
    })
  })

  it('lock 之后需要密钥的调用返回 locked', async () => {
    const api = makeApi()
    const { wrappedMk } = await api.createMasterKey(PASSWORD, KDF_PARAMS, SALT)
    await api.lock()

    await expect(api.createNodeKey(NODE_ID)).rejects.toMatchObject({
      code: 'locked',
    })
    await expect(api.nameToken(PARENT_ID, 'a.txt')).rejects.toMatchObject({
      code: 'locked',
    })
    await expect(
      api.encryptChunk(NODE_ID, CONTENT_ID, 0, 1, true, new Uint8Array([1])),
    ).rejects.toMatchObject({ code: 'locked' })

    // 重新解锁后恢复可用。
    await api.unlock(PASSWORD, KDF_PARAMS, SALT, wrappedMk)
    expect(await api.isLocked()).toBe(false)
    await expect(api.createNodeKey(NODE_ID)).resolves.toBeInstanceOf(Uint8Array)
  })

  it('不写入 localStorage、sessionStorage、IndexedDB 或 Cookie', async () => {
    const setItem = vi.spyOn(Storage.prototype, 'setItem')
    const indexedDbOpen = vi.fn()
    vi.stubGlobal('indexedDB', { open: indexedDbOpen })

    const cookieWrites: string[] = []
    const cookieDescriptor = Object.getOwnPropertyDescriptor(
      document,
      'cookie',
    )
    Object.defineProperty(document, 'cookie', {
      configurable: true,
      get: () => '',
      set: (value: string) => {
        cookieWrites.push(value)
      },
    })

    try {
      // 解锁、加解密、加锁全过程都不应持久化任何东西。
      const api = await unlockedApi()
      const plaintext = new TextEncoder().encode('persist me not')
      const ciphertext = await api.encryptChunk(
        NODE_ID,
        CONTENT_ID,
        0,
        1,
        true,
        plaintext,
      )
      await api.decryptChunk(NODE_ID, CONTENT_ID, 0, 1, true, ciphertext)
      await api.lock()

      expect(setItem).not.toHaveBeenCalled()
      expect(indexedDbOpen).not.toHaveBeenCalled()
      expect(cookieWrites).toEqual([])
      expect(localStorage.length).toBe(0)
      expect(sessionStorage.length).toBe(0)
    } finally {
      if (cookieDescriptor) {
        Object.defineProperty(document, 'cookie', cookieDescriptor)
      }
      vi.unstubAllGlobals()
      setItem.mockRestore()
    }
  })
})

