import { defineConfig, devices } from '@playwright/test'

/**
 * 容器化全链路验收（task 9.5）：不启动本地 webServer。
 *
 * 前置条件：deploy/docker-compose.e2e.yml 的 server 已启动并 /readyz 就绪。
 * 邮件由容器内 file 模式写到 named volume；E2E_MAIL_CONTAINER_SERVICE 让
 * e2e/mail.ts 通过 `docker compose exec` 读取，E2E_SENSITIVE_VALUES 指定
 * 日志扫描使用的敏感值清单。
 */
const baseUrl = process.env.E2E_BASE_URL ?? 'http://127.0.0.1:8080'

export default defineConfig({
  testDir: './e2e',
  testMatch: ['full-journey.spec.ts', 'security.spec.ts'],
  fullyParallel: false,
  workers: 1,
  timeout: 300_000,
  expect: { timeout: 30_000 },
  reporter: process.env.CI ? 'github' : 'list',
  use: {
    baseURL: baseUrl,
    trace: 'retain-on-failure',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
})
