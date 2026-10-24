import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import type { Plugin } from 'vite'
import { defineConfig } from 'vitest/config'

/**
 * 生产预览（`vite preview`）的严格 CSP。
 *
 * 与 design D12 / service-operations spec 一致：只允许同源脚本与 WASM 执行、
 * 禁止内联脚本；Worker 与 WASM 都是同源静态资源。
 */
const CONTENT_SECURITY_POLICY = [
  "default-src 'self'",
  "script-src 'self' 'wasm-unsafe-eval'",
  "worker-src 'self'",
  "style-src 'self'",
  "img-src 'self' data:",
  "font-src 'self'",
  "connect-src 'self'",
  "base-uri 'none'",
  "form-action 'self'",
  "frame-ancestors 'none'",
].join('; ')

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
  },
})
