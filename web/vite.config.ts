import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vitest/config'

// Vitest 与 Vite 共用同一份配置：`test` 段落只在 `vitest` 下生效。
// 注意：CSP 要求生产构建不产生内联脚本，因此这里不要引入会注入内联 script 的插件。
export default defineConfig({
  plugins: [react(), tailwindcss()],
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
    include: ['src/**/*.{test,spec}.{ts,tsx}'],
    restoreMocks: true,
  },
})
