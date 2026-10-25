import { expect, test } from '@playwright/test'

const CSP_SCRIPT_SRC = "script-src 'self' 'wasm-unsafe-eval'"

test('真实服务端返回严格 CSP 且所有公开页面没有 CSP 违规', async ({
  page,
}) => {
  await page.addInitScript(() => {
    const violations: string[] = []
    ;(window as unknown as { __cspViolations: string[] }).__cspViolations =
      violations
    document.addEventListener('securitypolicyviolation', (event) => {
      violations.push(`${event.violatedDirective} ${event.blockedURI}`)
    })
  })

  const response = await page.goto('/login')
  expect(response).not.toBeNull()

  const csp = response?.headers()['content-security-policy'] ?? ''
  expect(csp).toContain(CSP_SCRIPT_SRC)
  expect(csp).toContain("worker-src 'self'")
  expect(csp).toContain("frame-ancestors 'none'")

  await expect(
    page.getByRole('heading', { name: '登录' }),
  ).toBeVisible()

  // 登录前的品牌页面全部同源；登录后的文件/回收站/账号设置页由
  // files.spec.ts 与 account-flow.spec.ts 的 __cspViolations 断言覆盖。
  for (const path of [
    '/register',
    '/reset',
    '/security',
    '/verify-email',
    '/',
  ]) {
    const pageResponse = await page.goto(path)
    expect(pageResponse).not.toBeNull()
    const policy = pageResponse?.headers()['content-security-policy'] ?? ''
    expect(policy).toContain(CSP_SCRIPT_SRC)
    await page.waitForLoadState('domcontentloaded')
  }

  await expect(page).toHaveURL(/\/login$/)
  await page.goto('/security')
  await expect(
    page.getByRole('heading', { name: '安全与隐私' }),
  ).toBeVisible()

  const violations = await page.evaluate(
    () => (window as unknown as { __cspViolations: string[] }).__cspViolations,
  )
  expect(violations).toEqual([])
})
