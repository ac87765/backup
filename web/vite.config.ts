import { readFileSync } from 'node:fs'
import path from 'node:path'

import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import type { Plugin } from 'vite'
import { defineConfig } from 'vitest/config'

import { buildContentSecurityPolicy } from './src/security/csp.ts'

/**
 * 生产预览（`vite preview`）的严格 CSP。
 *
 * 基础策略与 Rust 服务端共享同一个文件 `csp/base-policy.txt`
 * （服务端用 `include_str!` 读取，并在启动时追加对象存储公网 origin）。
 * 这里只用于 vite preview 的开发预览；生产由 Axum 下发带 origin 的策略。
 */
const CONTENT_SECURITY_POLICY = buildContentSecurityPolicy(
  readFileSync(path.resolve(process.cwd(), '../csp/base-policy.txt'), 'utf8'),
)

function contentSecurityPolicyPlugin(): Plugin {
  return {
    name: 'backup-csp',
    // 只在 preview（生产构建产物）上加 CSP；dev server 的 HMR 不兼容严格 CSP。
    configurePreviewServer(server) {
      server.middlewares.use((_request, response, next) => {
        response.setHeader('Content-Security-Policy', CONTENT_SECURITY_POLICY)
        next()
      })
    },
  }
}

// Vitest 与 Vite 共用同一份配置：`test` 段落只在 `vitest` 下生效。
export default defineConfig({
  plugins: [react(), tailwindcss(), contentSecurityPolicyPlugin()],
  worker: {
    // Worker 使用 ES module（含顶层 await 初始化 WASM）。
    format: 'es',
  },
  build: {
    // 顶层 await 需要 es2022+。
    target: 'es2022',
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
    include: ['src/**/*.{test,spec}.{ts,tsx}'],
    restoreMocks: true,
    // KDF 与 4 MiB 块加解密测试在慢机器上可能超过 Vitest 默认的 5 秒。
    testTimeout: 30_000,
  },
})
