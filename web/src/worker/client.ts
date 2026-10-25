/**
 * 主线程使用的 Crypto Worker 客户端。
 *
 * 只创建 Worker 并用 Comlink 暴露的代理；**不 import WASM、不 import backend**，
 * 因此主线程 bundle 里不会有 WASM 或密钥代码。Worker 抛出的 `{ code, message }`
 * 载荷会在这里还原成 `CryptoApiError`。
 */
import { wrap, type Remote } from 'comlink'

import { CryptoApiError, isCryptoErrorPayload } from './crypto-api'
import type { WorkerApi } from './types'

let sharedWorker: WorkerApi | undefined

/** 创建 Crypto Worker，返回类型安全的 Comlink 代理。 */
export function createCryptoWorker(): WorkerApi {
  const worker = new Worker(new URL('./crypto.worker.ts', import.meta.url), {
    type: 'module',
  })
  return wrapApi(wrap<WorkerApi>(worker))
}

/** 应用内共享的 Worker 实例（避免 React StrictMode 重复创建）。 */
export function getCryptoWorker(): WorkerApi {
  if (!sharedWorker) {
    sharedWorker = createCryptoWorker()
  }
  return sharedWorker
}

/**
 * 把 Comlink 远程对象包装成普通方法：
 * - 每次调用都返回 Promise；
 * - Worker 抛出的错误载荷还原为 `CryptoApiError`（带稳定 code）。
 */
function wrapApi(remote: Remote<WorkerApi>): WorkerApi {
  const target = remote as unknown as WorkerApi

  return new Proxy(target, {
    get(inner, property, receiver) {
      const value = Reflect.get(inner, property, receiver) as unknown
      if (typeof value !== 'function') {
        return value
      }

      return async (...args: unknown[]) => {
        try {
          return await Reflect.apply(
            value as (...parameters: unknown[]) => Promise<unknown>,
            inner,
            args,
          )
        } catch (error) {
          if (isCryptoErrorPayload(error)) {
            throw new CryptoApiError(
              error.code,
              error.message,
              error.retryAfter,
            )
          }
          throw error
        }
      }
    },
  })
}



