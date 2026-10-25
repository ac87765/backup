import { randomUUID } from 'node:crypto'

import { expect, test, type Page } from '@playwright/test'

import { waitForVerificationToken } from './mail'

const OLD_PASSWORD = 'correct horse battery staple'
const NEW_PASSWORD = 'new correct horse battery staple'
const RESET_PASSWORD = 'reset correct horse battery staple'

async function fillRecoveryConfirmation(page: Page): Promise<void> {
  const display = await page.getByTestId('recovery-display').textContent()
  expect(display).toBeTruthy()
  const groups = (display ?? '').split('-').filter(Boolean)
  const inputs = page.locator('[data-testid^="recovery-confirm-"]')
  const count = await inputs.count()
  expect(count).toBeGreaterThan(0)
  for (let position = 0; position < count; position += 1) {
    const input = inputs.nth(position)
    const groupIndex = Number(await input.getAttribute('data-group-index'))
    await input.fill(groups[groupIndex] ?? '')
  }
}

test('注册 → 验证 → 登录 → 刷新解锁 → 改密 → 恢复重置 → 轮换恢复密钥', async ({
  page,
}) => {
  // Comlink 边界探针：记录主线程 → Worker 的消息，最后断言没有 32 字节 Uint8Array。
  await page.addInitScript(() => {
    const messages: unknown[] = []
    const violations: string[] = []
    ;(window as unknown as { __workerMessages: unknown[] }).__workerMessages =
      messages
    ;(window as unknown as { __cspViolations: string[] }).__cspViolations =
      violations
    document.addEventListener('securitypolicyviolation', (event) => {
      violations.push(`${event.violatedDirective} ${event.blockedURI}`)
    })
    // eslint-disable-next-line @typescript-eslint/unbound-method
    const original = Worker.prototype.postMessage
    Worker.prototype.postMessage = function postMessage(
      this: Worker,
      ...args: unknown[]
    ) {
      messages.push(args[0])
      Reflect.apply(original, this, args)
    }
  })

  const email = `e2e-${Date.now()}-${randomUUID()}@example.com`
  const registerRequests: string[] = []
  page.on('request', (request) => {
    if (request.url().includes('/api/v1/auth/register')) {
      registerRequests.push(request.url())
    }
  })

  // 弱密码：客户端阻止，且不发送注册请求。
  await page.goto('/register')
  await page.getByTestId('register-email').fill(email)
  await page.getByTestId('register-password').fill('password1234')
  await page.getByTestId('register-submit').click()
  await expect(page.getByRole('alert').first()).toBeVisible()
  expect(registerRequests).toEqual([])

  // 强密码注册 + 恢复密钥回填确认。
  await page.getByTestId('register-password').fill(OLD_PASSWORD)
  await page.getByTestId('register-submit').click()
  await expect(page.getByTestId('recovery-display')).toBeVisible()
  const recoveryDisplay = await page
    .getByTestId('recovery-display')
    .textContent()
  await fillRecoveryConfirmation(page)
  await page.getByRole('button', { name: '确认已保存并注册' }).click()
  await expect(
    page.getByRole('heading', { name: '注册成功' }),
  ).toBeVisible()

  // 从真实 file 模式邮件中解码 #token=，验证后片段被清除。
  const mailDir = process.env.E2E_MAIL_DIR
  expect(mailDir).toBeTruthy()
  const token = await waitForVerificationToken(mailDir as string, email)
  await page.goto(`/#token=${token}`)
  await expect(
    page.getByRole('heading', { name: '邮箱已验证' }),
  ).toBeVisible()
  await expect(page).not.toHaveURL(/#token=/)

  // 登录。
  await page.goto('/login')
  await page.getByTestId('login-email').fill(email)
  await page.getByTestId('login-password').fill(OLD_PASSWORD)
  await page.getByTestId('login-submit').click()
  await expect(page.getByTestId('home-email')).toHaveText(email)

  // 刷新页面后需要解锁；错误密码提示，正确密码恢复访问。
  await page.reload()
  await expect(page.getByRole('heading', { name: '解锁' })).toBeVisible()
  await page.getByTestId('unlock-password').fill('wrong password')
  await page.getByTestId('unlock-submit').click()
  await expect(page.getByRole('alert').first()).toContainText('邮箱或密码错误')
  await page.getByTestId('unlock-password').fill(OLD_PASSWORD)
  await page.getByTestId('unlock-submit').click()
  await expect(page.getByTestId('home-email')).toHaveText(email)

  // 直接打开受保护页面：先显示解锁界面，解锁后回到原本请求的改密页。
  await page.goto('/change-password')
  await expect(page.getByRole('heading', { name: '解锁' })).toBeVisible()
  await page.getByTestId('unlock-password').fill(OLD_PASSWORD)
  await page.getByTestId('unlock-submit').click()
  await expect(page.getByTestId('change-old-password')).toBeVisible()

  // 修改密码：旧密码错误时拒绝，正确时成功。
  await page.getByTestId('change-old-password').fill('wrong password')
  await page.getByTestId('change-new-password').fill(NEW_PASSWORD)
  await page.getByTestId('change-confirm-password').fill(NEW_PASSWORD)
  await page.getByTestId('change-submit').click()
  await expect(page.getByRole('alert').first()).toContainText('邮箱或密码错误')

  await page.getByTestId('change-old-password').fill(OLD_PASSWORD)
  await page.getByTestId('change-submit').click()
  await expect(page.getByRole('status')).toContainText('密码已修改')

  // 退出后用旧恢复密钥重置密码（页面切换使用应用内链接）。
  await page.getByRole('link', { name: '返回主页' }).click()
  await expect(page.getByTestId('home-email')).toHaveText(email)
  await page.getByRole('button', { name: '退出登录' }).click()
  await expect(
    page.getByRole('heading', { name: '端对端加密备份' }),
  ).toBeVisible()

  await page.goto('/reset')
  await page.getByTestId('reset-email').fill(email)
  await page.getByTestId('reset-recovery-key').fill(recoveryDisplay ?? '')
  await page.getByTestId('reset-new-password').fill(RESET_PASSWORD)
  await page.getByTestId('reset-confirm-password').fill(RESET_PASSWORD)
  await page.getByTestId('reset-submit').click()
  await expect(page.getByRole('status')).toContainText('密码已重置')

  // 用新密码登录。
  await page.goto('/login')
  await page.getByTestId('login-email').fill(email)
  await page.getByTestId('login-password').fill(RESET_PASSWORD)
  await page.getByTestId('login-submit').click()
  await expect(page.getByTestId('home-email')).toHaveText(email)

  // 轮换恢复密钥：需要当前密码，确认后旧恢复密钥失效。
  await page.getByRole('link', { name: '轮换恢复密钥' }).click()
  await page.getByTestId('rotate-current-password').fill(RESET_PASSWORD)
  await page.getByTestId('rotate-prepare').click()
  await expect(page.getByTestId('recovery-display')).toBeVisible()
  await fillRecoveryConfirmation(page)
  await page.getByRole('button', { name: '确认并轮换' }).click()
  await expect(page.getByRole('status')).toContainText('恢复密钥已轮换')

  const keyLeaks = await page.evaluate(() => {
    const leaks: string[] = []
    const visit = (value: unknown, path: string): void => {
      if (value instanceof Uint8Array) {
        if (value.length === 32) {
          leaks.push(path)
        }
        return
      }
      if (Array.isArray(value)) {
        value.forEach((item, index) => visit(item, `${path}[${index}]`))
        return
      }
      if (typeof value === 'object' && value !== null) {
        for (const [key, nested] of Object.entries(value)) {
          visit(nested, `${path}.${key}`)
        }
      }
    }
    const messages = (window as unknown as { __workerMessages: unknown[] })
      .__workerMessages
    messages.forEach((message, index) => visit(message, `message[${index}]`))
    return leaks
  })
  expect(keyLeaks).toEqual([])

  const cspViolations = await page.evaluate(
    () => (window as unknown as { __cspViolations: string[] }).__cspViolations,
  )
  expect(cspViolations).toEqual([])
})
