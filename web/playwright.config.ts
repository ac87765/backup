import path from 'node:path'

import { defineConfig, devices } from '@playwright/test'

/**
 * 真实后端 E2E：构建前端 → `server serve` 托管 web/dist 与 API。
 * 邮件使用 file 模式写入 E2E_MAIL_DIR，测试从 .eml 解码令牌。
 */
const baseUrl = process.env.E2E_BASE_URL ?? 'http://127.0.0.1:4174'
const mailDir =
  process.env.E2E_MAIL_DIR ?? path.resolve(process.cwd(), 'e2e-tmp', 'mail')
process.env.E2E_MAIL_DIR = mailDir

export default defineConfig({
  testDir: './e2e',
  fullyParallel: false,
  timeout: 180_000,
  expect: { timeout: 20_000 },
  reporter: process.env.CI ? 'github' : 'list',
  use: {
    baseURL: baseUrl,
    trace: 'retain-on-failure',
    // design D10：E2E 不依赖动画完成时机。
    reducedMotion: 'reduce',
  },
  webServer: {
    // 本机沙箱下可先手动 `npm run build`，再用 E2E_SKIP_BUILD=1 跳过重复构建。
    command:
      process.env.E2E_SKIP_BUILD === '1'
        ? 'node e2e/serve.mjs'
        : 'npm run e2e:serve',
    url: baseUrl,
    reuseExistingServer: !process.env.CI,
    timeout: 300_000,
    env: {
      E2E_BASE_URL: baseUrl,
      E2E_MAIL_DIR: mailDir,
      ...(process.env.E2E_DATABASE_URL
        ? { E2E_DATABASE_URL: process.env.E2E_DATABASE_URL }
        : {}),
      ...(process.env.E2E_STORAGE_ENDPOINT
        ? { E2E_STORAGE_ENDPOINT: process.env.E2E_STORAGE_ENDPOINT }
        : {}),
      ...(process.env.E2E_SERVER_BIN
        ? { E2E_SERVER_BIN: process.env.E2E_SERVER_BIN }
        : {}),
      ...(process.env.SKIP_WASM_BUILD
        ? { SKIP_WASM_BUILD: process.env.SKIP_WASM_BUILD }
        : {}),
      ...(process.env.E2E_USER_WRITE_RATE_LIMIT
        ? { E2E_USER_WRITE_RATE_LIMIT: process.env.E2E_USER_WRITE_RATE_LIMIT }
        : {}),
    },
  },
  projects: [
    {
      name: 'chromium',
      use: { ...devices['Desktop Chrome'] },
      testIgnore: /admin-console\.spec\.ts/,
    },
    // 管理后台测试会临时修改全局注册模式；必须在其他场景全部结束后单独运行，
    // 否则并行注册的测试账号会落入待审核状态。
    {
      name: 'admin',
      use: { ...devices['Desktop Chrome'] },
      testMatch: /admin-console\.spec\.ts/,
      dependencies: ['chromium'],
    },
  ],
})
