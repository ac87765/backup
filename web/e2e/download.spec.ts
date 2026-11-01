import { expect, test, type Page } from '@playwright/test'

const WINDOWS_UA =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36'
const IPHONE_UA =
  'Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1'

const MANIFEST = {
  schema: 1,
  version: '0.2.0',
  released_at: '2026-11-05T10:00:00Z',
  min_supported: '0.1.0',
  notes: ['支持微信备份', '托盘菜单修复'],
  history: [
    {
      version: '0.1.0',
      released_at: '2026-11-01T10:00:00Z',
      notes: ['首个版本'],
    },
  ],
  platforms: {
    'windows-x64': {
      file: 'EncryptedBackup-0.2.0-windows-x64.exe',
      size: 15_400_000,
      sha256:
        '8f3c2a91d0e74b6c5a1f9e2d7c4b8a3f6e5d0c9b8a7f6e5d4c3b2a1f0e9d8c7b',
    },
    'macos-universal': {
      file: 'EncryptedBackup-0.2.0-macos.dmg',
      size: 38_500_000,
      sha256:
        '2b7e4d1a9c8f3e6b5d0a7c4f1e8b2d9a6c3f0e7b4d1a8c5f2e9b6d3a0c7f4e1b',
    },
  },
}

async function mockManifest(page: Page) {
  await page.route('**/downloads/latest.json', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json; charset=utf-8',
      body: JSON.stringify(MANIFEST),
    })
  })
}

async function collectCspViolations(page: Page) {
  await page.addInitScript(() => {
    const violations: string[] = []
    ;(window as unknown as { __cspViolations: string[] }).__cspViolations =
      violations
    document.addEventListener('securitypolicyviolation', (event) => {
      violations.push(`${event.violatedDirective} ${event.blockedURI}`)
    })
  })
}

test.describe('公开下载页', () => {
  test.use({ userAgent: WINDOWS_UA })

  test('未登录可访问 Windows 下载页，不加载 wasm / worker，严格 CSP 下无违规', async ({
    page,
  }) => {
    await collectCspViolations(page)
    await mockManifest(page)

    const wasmRequests: string[] = []
    const workerRequests: string[] = []
    page.on('request', (request) => {
      const url = request.url()
      if (url.endsWith('.wasm')) {
        wasmRequests.push(url)
      }
      if (url.includes('crypto.worker')) {
        workerRequests.push(url)
      }
    })

    const response = await page.goto('/download')
    expect(response).not.toBeNull()
    const csp = response?.headers()['content-security-policy'] ?? ''
    expect(csp).toContain("script-src 'self' 'wasm-unsafe-eval'")
    expect(csp).toContain("worker-src 'self'")
    expect(csp).toContain("frame-ancestors 'none'")

    await expect(
      page.getByRole('heading', { name: /在电脑上自动加密备份.*你的文件/ }),
    ).toBeVisible()
    await expect(page.getByTestId('download-windows')).toContainText(
      '下载 Windows 版',
    )
    await expect(page.getByText('也有 macOS 版')).toBeVisible()
    await expect(page.getByText(/版本 0\.2\.0/)).toBeVisible()
    await expect(page.getByTestId('download-manifest-error')).toHaveCount(0)
    await expect(page.getByTestId('download-first-open')).toContainText(
      'Windows 已保护你的电脑',
    )

    // Playwright 配置开启 reducedMotion：进入动画与浮动动画都应关闭。
    await expect(page.locator('.download-shot')).toHaveCSS(
      'animation-name',
      'none',
    )
    await expect(
      page.getByRole('heading', { level: 1 }),
    ).toHaveCSS('animation-name', 'none')

    const violations = await page.evaluate(
      () =>
        (window as unknown as { __cspViolations: string[] }).__cspViolations,
    )
    expect(violations).toEqual([])
    expect(wasmRequests).toEqual([])
    expect(workerRequests).toEqual([])
  })
})

test.describe('手机宽度', () => {
  test.use({
    userAgent: IPHONE_UA,
    viewport: { width: 390, height: 844 },
    isMobile: true,
    hasTouch: true,
  })

  test('显示电脑端提示与复制链接，页面无横向滚动', async ({ page }) => {
    await mockManifest(page)
    await page.goto('/download')

    await expect(page.getByTestId('copy-page-link')).toBeVisible()
    await expect(
      page.getByText(/客户端适用于 Windows 与 macOS 电脑/),
    ).toBeVisible()
    await expect(page.getByTestId('download-windows')).toHaveCount(0)
    await expect(page.getByTestId('platform-windows')).toBeVisible()
    await expect(page.getByTestId('platform-iphone')).toContainText('即将推出')

    const overflow = await page.evaluate(() => ({
      scrollWidth: document.documentElement.scrollWidth,
      clientWidth: document.documentElement.clientWidth,
    }))
    expect(overflow.scrollWidth).toBeLessThanOrEqual(overflow.clientWidth)
    await expect(page.getByTestId('changelog')).toContainText('首个版本')
  })
})
