/**
 * 隔离测试：确保只有 Worker 加载 WASM。
 *
 * ESLint 的 `no-restricted-imports` 已禁止主线程 import WASM/backend；这里再用测试
 * 扫描源码，防止有人绕过 lint 或在本文件之外意外引入。
 */
import { readdirSync, readFileSync } from 'node:fs'
import { join, relative } from 'node:path'
import { resolve } from 'node:path'

import { describe, expect, it } from 'vitest'

// Vitest 的 cwd 是 web/；避免依赖 import.meta.url 的协议（jsdom 下不是 file:）。
const SRC_ROOT = resolve(process.cwd(), 'src')
const WORKER_ROOT = resolve(process.cwd(), 'src/worker')
const WASM_IMPORT = /generated\/crypto-wasm|crypto-backend/

function listSourceFiles(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name)
    if (entry.isDirectory()) {
      return listSourceFiles(path)
    }
    return /\.(ts|tsx)$/.test(entry.name) ? [path] : []
  })
}

function isTestFile(path: string): boolean {
  return /\.(test|spec)\.(ts|tsx)$/.test(path)
}

describe('WASM 加载隔离', () => {
  it('主线程源码不 import WASM 或 crypto-backend', () => {
    const offenders = listSourceFiles(SRC_ROOT)
      .filter((path) => !path.startsWith(WORKER_ROOT))
      .filter((path) => !isTestFile(path))
      .filter((path) => WASM_IMPORT.test(readFileSync(path, 'utf8')))
      .map((path) => relative(SRC_ROOT, path))

    expect(offenders).toEqual([])
  })

  it('Worker 内部只有 backend 与测试初始化直接加载 WASM', () => {
    const importers = listSourceFiles(WORKER_ROOT)
      .filter((path) => !isTestFile(path))
      .filter((path) => /generated\/crypto-wasm/.test(readFileSync(path, 'utf8')))
      .map((path) => relative(WORKER_ROOT, path))
      .sort()

    expect(importers).toEqual(['crypto-backend.ts', 'test-init.ts'])
  })

  it('主线程客户端不 import backend', () => {
    const client = readFileSync(join(WORKER_ROOT, 'client.ts'), 'utf8')
    expect(client).not.toMatch(/crypto-backend/)
    expect(client).not.toMatch(/generated\/crypto-wasm/)
  })
})

