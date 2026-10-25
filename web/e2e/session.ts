import { expect, type Page } from '@playwright/test'

import { waitForVerificationToken } from './mail'

/** 新增 UI E2E 共用的账号与文件操作步骤。 */
export const UI_PASSWORD = 'ui redesign correct horse battery staple'

export async function fillRecoveryConfirmation(page: Page): Promise<void> {
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

export async function registerVerifyAndLogin(
  page: Page,
  email: string,
  password = UI_PASSWORD,
): Promise<void> {
  await page.goto('/register')
  await page.getByTestId('register-email').fill(email)
  await page.getByTestId('register-password').fill(password)
  await page.getByTestId('register-submit').click()
  await expect(page.getByTestId('recovery-display')).toBeVisible()
  await fillRecoveryConfirmation(page)
  await page.getByRole('button', { name: '确认已保存并注册' }).click()
  await expect(
    page.getByRole('heading', { name: '注册成功' }),
  ).toBeVisible()

  const mailDir = process.env.E2E_MAIL_DIR
  expect(mailDir).toBeTruthy()
  const token = await waitForVerificationToken(mailDir as string, email)
  await page.goto(`/#token=${token}`)
  await expect(
    page.getByRole('heading', { name: '邮箱已验证' }),
  ).toBeVisible()

  await page.goto('/login')
  await page.getByTestId('login-email').fill(email)
  await page.getByTestId('login-password').fill(password)
  await page.getByTestId('login-submit').click()
  await expect(page.getByTestId('home-email')).toHaveText(email)
  await expect(page.getByRole('heading', { name: '全部文件' })).toBeVisible()
}

export function fileRow(page: Page, name: string) {
  return page.locator(`[data-testid="file-row"][data-name="${name}"]`)
}

export async function createFolder(page: Page, name: string): Promise<void> {
  await page.getByTestId('upload-menu').click()
  await page.getByTestId('new-folder').click()
  await page.getByTestId('folder-name').fill(name)
  await page.getByTestId('create-folder-submit').click()
  await expect(page.getByTestId('folder-name')).toHaveCount(0)
  await expect(fileRow(page, name)).toBeVisible()
}

export async function openMoreMenu(page: Page, name: string): Promise<void> {
  await fileRow(page, name)
    .getByRole('button', { name: `更多 ${name}` })
    .click()
}

export async function unlockWithPassword(
  page: Page,
  password = UI_PASSWORD,
): Promise<void> {
  await expect(page.getByRole('heading', { name: '解锁' })).toBeVisible()
  await page.getByTestId('unlock-password').fill(password)
  await page.getByTestId('unlock-submit').click()
}

export async function uploadFile(
  page: Page,
  name: string,
  buffer: Buffer,
  mimeType = 'application/octet-stream',
): Promise<void> {
  await page.setInputFiles('[data-testid="upload-input"]', {
    name,
    mimeType,
    buffer,
  })
  const item = page
    .locator(`[data-testid="transfer-item"][data-name="${name}"]`)
    .last()
  await expect(item).toHaveAttribute('data-status', 'completed', {
    timeout: 120_000,
  })
  await expect(fileRow(page, name)).toBeVisible()
}
