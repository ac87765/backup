import { defineConfig, devices } from '@playwright/test'

/**
 * Playwright 冒烟测试：验证**生产构建**在严格 CSP 下能加载 WASM 并完成一次加解密。
 *
 * `webServer` 会用 `VITE_SELFTEST=1` 构建，并通过 `vite preview` 提供
 * `script-src 'self' 'wasm-unsafe-eval'` 的 CSP 响应头。
 */
export default defineConfig({
  testDir: './e2e',
  fullyParallel: false,
  timeout: 120_000,
  expect: { timeout: 15_000 },
  reporter: process.env.CI ? 'github' : 'list',
  use: {
    baseURL: 'http://127.0.0.1:4173',
    trace: 'retain-on-failure',
  },
  webServer: {
    command: 'npm run e2e:preview',
    url: 'http://127.0.0.1:4173',
    reuseExistingServer: !process.env.CI,
    timeout: 240_000,
    env: {
      VITE_SELFTEST: '1',
      // CI 使用 rust job 上传的 WASM artifact；确保 webServer 子进程也能看到该开关。
      ...(process.env.SKIP_WASM_BUILD
        ? { SKIP_WASM_BUILD: process.env.SKIP_WASM_BUILD }
        : {}),
    },
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
})
