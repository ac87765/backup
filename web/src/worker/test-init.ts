/**
 * 仅供 Vitest 使用：在 Node 里同步初始化 WASM。
 *
 * 生产路径见 `crypto-backend.ts` 的 `initCryptoWasm()`（浏览器 fetch 静态资源）；
 * 测试用 `initSync` + 直接读取 .wasm 文件，避免在 jsdom 中伪造网络。
 */
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

import { initSync } from '../generated/crypto-wasm/crypto_wasm.js'

let initialized = false

/** 幂等初始化（每个测试文件共享同一模块实例）。 */
export function ensureWasmForTests(): void {
  if (initialized) {
    return
  }
  // Vitest 的 cwd 是 web/；用相对项目根的路径避免依赖 import.meta.url 的协议。
  const wasmPath = resolve(
    process.cwd(),
    'src/generated/crypto-wasm/crypto_wasm_bg.wasm',
  )
  initSync({ module: readFileSync(wasmPath) })
  initialized = true
}
