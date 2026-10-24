import { expect, test } from '@playwright/test'

const CSP_SCRIPT_SRC = "script-src 'self' 'wasm-unsafe-eval'"

test('生产构建在严格 CSP 下加载 WASM 并完成一次加解密', async ({ page }) => {
  // 记录所有 CSP 违规；断言页面没有触发任何一条。
  await page.addInitScript(() => {
    const violations: string[] = []
    ;(window as unknown as { __cspViolations: string[] }).__cspViolations =
      violations
    document.addEventListener('securitypolicyviolation', (event) => {
      violations.push(`${event.violatedDirective} ${event.blockedURI}`)
    })
  })

  const response = await page.goto('/?selftest=1')
  expect(response).not.toBeNull()

  const csp = response?.headers()['content-security-policy'] ?? ''
  expect(csp).toContain(CSP_SCRIPT_SRC)

  // 主线程 → Comlink → Worker → WASM：创建 MK、解锁、分块加解密、锁定错误码。
  await expect(page.getByTestId('selftest-status')).toHaveText('ok', {
    timeout: 90_000,
  })

  const violations = await page.evaluate(
    () => (window as unknown as { __cspViolations: string[] }).__cspViolations,
  )
  expect(violations).toEqual([])
})
