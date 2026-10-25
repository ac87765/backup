import { randomUUID } from 'node:crypto'

import { expect, test, type Page } from '@playwright/test'

import { grantAdmin } from './admin'
import { waitForVerificationToken } from './mail'
import { UI_PASSWORD, fillRecoveryConfirmation, uploadFile } from './session'

/**
 * 管理后台 E2E（add-admin-console task 4.4）。
 *
 * 本地 E2E 的 serve.mjs 会把注册模式设为 open，使既有场景可直接上传；
 * 本测试临时切到 approval 验证审批流程，并在结束时恢复 open。
 */

function uniqueEmail(prefix: string): string {
  return `${prefix}-${randomUUID()}@example.com`
}

async function registerAndVerify(page: Page, email: string): Promise<void> {
  await page.goto('/register')
  await page.getByTestId('register-email').fill(email)
  await page.getByTestId('register-password').fill(UI_PASSWORD)
  await page.getByTestId('register-submit').click()
  await expect(page.getByTestId('recovery-display')).toBeVisible()
  await fillRecoveryConfirmation(page)
  await page.getByRole('button', { name: '确认已保存并注册' }).click()
  await expect(page.getByRole('heading', { name: '注册成功' })).toBeVisible()

  const mailDir = process.env.E2E_MAIL_DIR
  expect(mailDir).toBeTruthy()
  const token = await waitForVerificationToken(mailDir as string, email)
  await page.goto(`/#token=${token}`)
  await expect(page.getByRole('heading', { name: '邮箱已验证' })).toBeVisible()
}

async function login(page: Page, email: string): Promise<void> {
  await page.goto('/login')
  await page.getByTestId('login-email').fill(email)
  await page.getByTestId('login-password').fill(UI_PASSWORD)
  await page.getByTestId('login-submit').click()
}

/**
 * 整页导航（goto/reload）后密钥已从 Worker 内存清除，登录态页面会先要求解锁。
 * 这里在需要时输入密码解锁，然后等待外壳（或等待审核页）出现。
 */
async function gotoUnlocked(page: Page, path?: string): Promise<void> {
  if (path) {
    await page.goto(path)
  } else {
    await page.reload()
  }
  const unlock = page.getByTestId('unlock-password')
  const ready = page
    .getByTestId('account-menu-trigger')
    .or(page.getByTestId('pending-page'))
    // 管理后台使用自己的侧边栏，没有顶部账号菜单。
    .or(page.getByRole('navigation', { name: '管理后台导航' }))
  await expect(unlock.or(ready).first()).toBeVisible()
  if (await unlock.isVisible()) {
    await unlock.fill(UI_PASSWORD)
    await page.getByTestId('unlock-submit').click()
    await expect(ready.first()).toBeVisible()
  }
}

async function setRegistrationMode(
  page: Page,
  mode: 'approval' | 'open' | 'closed',
): Promise<void> {
  await gotoUnlocked(page, '/admin/settings')
  await page.getByTestId(`admin-registration-mode-${mode}`).check()
  await page.getByTestId('admin-settings-save').click()
  await expect(page.getByTestId('toast')).toContainText('设置已保存')
}

test('管理后台：审批 50 GB → 用户可上传 → 停用显示原因 → 关闭注册 → 审计', async ({
  page,
  browser,
}) => {
  const adminEmail = uniqueEmail('e2e-admin')
  const userEmail = uniqueEmail('e2e-pending')

  // 1. 通过命令行授予管理员（HTTP 没有任何提权路径）。
  await registerAndVerify(page, adminEmail)
  await login(page, adminEmail)
  await expect(page.getByRole('heading', { name: '全部文件' })).toBeVisible()
  grantAdmin(adminEmail)
  await gotoUnlocked(page)
  await page.getByTestId('account-menu-trigger').click()
  await page.getByTestId('account-admin').click()
  await expect(page.getByTestId('admin-overview')).toBeVisible()
  await expect(page.getByText('管理员无法查看用户的文件名或内容')).toBeVisible()

  await setRegistrationMode(page, 'approval')
  const userContext = await browser.newContext()
  try {
    // 2. 新用户注册并验证后看到等待审核页。
    const userPage = await userContext.newPage()
    await registerAndVerify(userPage, userEmail)
    await login(userPage, userEmail)
    await expect(userPage.getByTestId('pending-page')).toBeVisible()
    await expect(
      userPage.getByRole('heading', { name: '正在等待管理员审核' }),
    ).toBeVisible()

    // 3. 管理员批准并选择 50 GB；用户刷新后可以上传。
    await gotoUnlocked(page, '/admin/users?status=pending')
    const row = page
      .getByTestId('admin-user-row')
      .filter({ hasText: userEmail })
    await expect(row).toBeVisible()
    await row.getByTestId('admin-user-approve').click()
    await page.getByLabel('50 GB').check()
    await page.getByTestId('admin-approve-confirm').click()
    await expect(row).toHaveCount(0)

    await gotoUnlocked(userPage)
    await expect(
      userPage.getByRole('heading', { name: '全部文件' }),
    ).toBeVisible()
    await uploadFile(userPage, 'admin-approved.txt', Buffer.from('approved'))

    // 4. 停用：旧会话失效，重新登录显示原因。
    await gotoUnlocked(page, '/admin/users?status=active')
    await page
      .getByTestId('admin-user-row')
      .filter({ hasText: userEmail })
      .click()
    await expect(page.getByTestId('admin-detail-drawer')).toBeVisible()
    await page.getByTestId('admin-suspend').click()
    await page.getByTestId('admin-reason').fill('E2E 测试停用原因')
    await page.getByRole('checkbox', { name: /我确认要停用该账号/ }).check()
    await page.getByTestId('admin-confirm-danger').click()
    await expect(page.getByTestId('toast')).toContainText('已停用')

    await userPage.reload()
    await expect(userPage.getByRole('heading', { name: '登录' })).toBeVisible()
    await login(userPage, userEmail)
    await expect(userPage.getByRole('alert')).toContainText(
      '账号已被停用：E2E 测试停用原因',
    )

    // 5. 关闭注册后注册页显示提示。
    await setRegistrationMode(page, 'closed')
    const closedPage = await userContext.newPage()
    await closedPage.goto('/register')
    await closedPage
      .getByTestId('register-email')
      .fill(uniqueEmail('e2e-closed'))
    await closedPage.getByTestId('register-password').fill(UI_PASSWORD)
    await closedPage.getByTestId('register-submit').click()
    await expect(closedPage.getByTestId('recovery-display')).toBeVisible()
    await fillRecoveryConfirmation(closedPage)
    await closedPage.getByRole('button', { name: '确认已保存并注册' }).click()
    await expect(closedPage.getByRole('alert')).toContainText('暂不开放注册')

    // 6. 审计日志可见，并且可以按用户筛选。
    await gotoUnlocked(page, '/admin/audit')
    await expect(page.getByTestId('admin-audit-row').first()).toContainText(
      '修改设置',
    )
    await expect(
      page.getByTestId('admin-audit-row').filter({ hasText: '批准账号' }),
    ).toBeVisible()
  } finally {
    // 无论成功与否都恢复 open，避免失败时连带影响其他 E2E 场景。
    await setRegistrationMode(page, 'open')
    await userContext.close()
  }
})
