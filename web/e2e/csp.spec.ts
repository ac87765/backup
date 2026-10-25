import { expect, test } from '@playwright/test'

const CSP_SCRIPT_SRC = "script-src 'self' 'wasm-unsafe-eval'"

test('真实服务端返回严格 CSP 且页面没有 CSP 违规', async ({ page }) => {
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
    page.getByRole('heading', { name: '端对端加密备份' }),
  ).toBeVisible()

  const violations = await page.evaluate(
    () => (window as unknown as { __cspViolations: string[] }).__cspViolations,
  )
  expect(violations).toEqual([])
})
