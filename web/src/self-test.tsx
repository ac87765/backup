/**
 * 仅用于 Playwright CSP 冒烟测试的自检页（`?selftest=1` + `VITE_SELFTEST=1` 构建）。
 *
 * 它走完整的主线程 → Comlink → Worker → WASM 路径：创建 MK、解锁、节点密钥、
 * 块加解密，并验证 `lock()` 之后通过 Comlink 拿到的错误码是 `locked`。
 */
import { createCryptoWorker } from './worker/client'

const KDF_PARAMS =
  '{"algorithm":"argon2id","m_cost_kib":65536,"t_cost":3,"p_cost":1}'

export function renderSelfTest(root: HTMLElement): void {
  const status = document.createElement('p')
  status.dataset.testid = 'selftest-status'
  status.textContent = 'running'
  root.replaceChildren(status)

  const api = createCryptoWorker()

  void (async () => {
    const salt = crypto.getRandomValues(new Uint8Array(16))
    const { wrappedMk } = await api.createMasterKey(
      'self-test password',
      KDF_PARAMS,
      salt,
    )
    await api.lock()
    await api.unlock('self-test password', KDF_PARAMS, salt, wrappedMk)

    const nodeId = crypto.randomUUID()
    const contentId = crypto.randomUUID()
    const wrappedNodeKey = await api.createNodeKey(nodeId)
    await api.loadNodeKey(nodeId, wrappedNodeKey)

    const plaintext = new TextEncoder().encode('CSP smoke test')
    const ciphertext = await api.encryptChunk(
      nodeId,
      contentId,
      0,
      1,
      true,
      plaintext,
    )
    const decrypted = await api.decryptChunk(
      nodeId,
      contentId,
      0,
      1,
      true,
      ciphertext,
    )
    if (new TextDecoder().decode(decrypted) !== 'CSP smoke test') {
      throw new Error('加解密往返结果不一致')
    }

    // 通过 Comlink 验证锁定错误码能完整到达主线程。
    await api.lock()
    try {
      await api.createNodeKey(nodeId)
      throw new Error('lock 之后仍能创建节点密钥')
    } catch (error) {
      if ((error as { code?: string }).code !== 'locked') {
        throw error
      }
    }

    status.textContent = 'ok'
  })().catch((error: unknown) => {
    status.textContent = 'error'
    status.dataset.error =
      error instanceof Error ? error.message : JSON.stringify(error)
  })
}



