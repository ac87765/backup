import { randomUUID } from 'node:crypto'

import { expect, test, type Page } from '@playwright/test'
import { Client } from 'pg'

import { waitForVerificationToken } from './mail'

const PASSWORD = 'folder browser correct horse battery staple'

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

async function registerVerifyAndLogin(
  page: Page,
  email: string,
): Promise<void> {
  await page.goto('/register')
  await page.getByTestId('register-email').fill(email)
  await page.getByTestId('register-password').fill(PASSWORD)
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
  await page.getByTestId('login-password').fill(PASSWORD)
  await page.getByTestId('login-submit').click()
  await expect(page.getByTestId('home-email')).toHaveText(email)
}

function fileRow(page: Page, name: string) {
  return page.locator(`[data-testid="file-row"][data-name="${name}"]`)
}

async function createFolder(page: Page, name: string): Promise<void> {
  await page.getByTestId('upload-menu').click()
  await page.getByTestId('new-folder').click()
  await page.getByTestId('folder-name').fill(name)
  await page.getByTestId('create-folder-submit').click()
  await expect(page.getByTestId('folder-name')).toHaveCount(0)
  await expect(fileRow(page, name)).toBeVisible()
}

async function openFolder(page: Page, name: string): Promise<void> {
  await fileRow(page, name)
    .getByRole('button', { name: `打开 ${name}` })
    .click()
  await expect(page.getByTestId('breadcrumb')).toContainText(name)
  await expect(fileRow(page, name)).toHaveCount(0)
}

function trashItem(page: Page, name: string) {
  return page.locator(`[data-testid="trash-item"][data-name="${name}"]`)
}

test('文件浏览器：多级文件夹 → 重命名 → 移动 → 同名冲突 → 回收站 → 用量', async ({
  page,
}) => {
  // 第 6 组的 Worker → 主线程边界探针：文件浏览流程也不得返回密钥。
  await page.addInitScript(() => {
    const replies: unknown[] = []
    const violations: string[] = []
    ;(window as unknown as { __workerReplies: unknown[] }).__workerReplies =
      replies
    ;(window as unknown as { __cspViolations: string[] }).__cspViolations =
      violations
    document.addEventListener('securitypolicyviolation', (event) => {
      violations.push(`${event.violatedDirective} ${event.blockedURI}`)
    })

    const findKeyLeaks = (roots: unknown[]): string[] => {
      const leaks: string[] = []
      const visit = (value: unknown, path: string): void => {
        if (value instanceof Uint8Array || value instanceof ArrayBuffer) {
          if (value.byteLength === 32) {
            leaks.push(path)
          }
          return
        }
        if (typeof value === 'string') {
          if (/^[0-9a-fA-F]{64}$/.test(value)) {
            leaks.push(`${path}:hex64`)
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
      roots.forEach((root, index) => visit(root, `message[${index}]`))
      return leaks
    }
    ;(window as unknown as {
      __findKeyLeaks: (roots: unknown[]) => string[]
    }).__findKeyLeaks = findKeyLeaks

    const NativeWorker = window.Worker
    const RecordingWorker = function (
      this: Worker,
      ...args: unknown[]
    ) {
      const worker = new NativeWorker(
        ...(args as ConstructorParameters<typeof Worker>),
      )
      worker.addEventListener('message', (event) => {
        replies.push(event.data)
      })
      return worker
    } as unknown as typeof Worker
    RecordingWorker.prototype = NativeWorker.prototype
    ;(window as unknown as { Worker: typeof Worker }).Worker = RecordingWorker
  })

  const email = `files-${Date.now()}-${randomUUID()}@example.com`
  const secretPrefix = `机密文件夹-${randomUUID()}`
  const folderA = `${secretPrefix}-A`
  const folderB = `${secretPrefix}-B`
  const folderC = `${secretPrefix}-C`
  const folderRenamed = `${secretPrefix}-C2`

  await registerVerifyAndLogin(page, email)
  await expect(page.getByRole('heading', { name: '我的文件' })).toBeVisible()
  await expect(
    page.getByRole('link', { name: '全部文件' }),
  ).toHaveAttribute('aria-current', 'page')
  await expect(page.getByTestId('usage-summary')).toContainText('已用')

  await createFolder(page, folderA)
  await expect(fileRow(page, folderB)).toHaveCount(0)

  // 多级：A → B → C。
  await openFolder(page, folderA)
  await createFolder(page, folderB)
  await openFolder(page, folderB)
  await createFolder(page, folderC)

  // 重命名 C → C2。
  await fileRow(page, folderC)
    .getByRole('button', { name: `更多 ${folderC}` })
    .click()
  await page.getByRole('menuitem', { name: `重命名 ${folderC}` }).click()
  await page.getByTestId('rename-name').fill(folderRenamed)
  await page.getByTestId('rename-submit').click()
  await expect(page.getByTestId('rename-name')).toHaveCount(0)
  await expect(fileRow(page, folderRenamed)).toBeVisible()
  await expect(fileRow(page, folderC)).toHaveCount(0)

  // 把 C2 移动到 A：选择器从根开始，进入 A 后选择当前文件夹。
  await fileRow(page, folderRenamed)
    .getByRole('button', { name: `更多 ${folderRenamed}` })
    .click()
  await page.getByRole('menuitem', { name: `移动 ${folderRenamed}` }).click()
  await expect(page.getByTestId('folder-picker')).toBeVisible()
  await page
    .locator(`[data-testid="picker-folder"][data-name="${folderA}"]`)
    .click()
  await page.getByTestId('picker-select-current').click()
  await expect(page.getByTestId('folder-picker')).toHaveCount(0)

  // 回到根，进入 A，应看到 C2。
  await page
    .getByTestId('breadcrumb')
    .getByRole('button', { name: '我的文件' })
    .click()
  await openFolder(page, folderA)
  await expect(fileRow(page, folderRenamed)).toBeVisible()

  // 同名冲突：在 A 下再创建 C2，服务端名称令牌冲突，页面显示“同名”。
  await page.getByTestId('upload-menu').click()
  await page.getByTestId('new-folder').click()
  await page.getByTestId('folder-name').fill(folderRenamed)
  await page.getByTestId('create-folder-submit').click()
  await expect(page.getByRole('alert').first()).toContainText('同名')
  await page.getByRole('button', { name: '取消' }).click()

  // 移入回收站 → 恢复。
  await fileRow(page, folderRenamed)
    .getByRole('button', { name: `更多 ${folderRenamed}` })
    .click()
  await page.getByRole('menuitem', { name: `删除 ${folderRenamed}` }).click()
  await expect(fileRow(page, folderRenamed)).toHaveCount(0)
  await page.getByTestId('open-trash').click()
  await expect(trashItem(page, folderRenamed)).toBeVisible()
  await trashItem(page, folderRenamed)
    .getByRole('button', { name: `恢复 ${folderRenamed}` })
    .click()
  await expect(trashItem(page, folderRenamed)).toHaveCount(0)
  // 回收站是独立视图：回到文件页并重新进入 A 验证恢复后的位置。
  await page.getByRole('link', { name: '全部文件' }).click()
  await expect(page.getByRole('heading', { name: '我的文件' })).toBeVisible()
  await openFolder(page, folderA)
  await expect(fileRow(page, folderRenamed)).toBeVisible()

  // 再删除并永久删除。
  await fileRow(page, folderRenamed)
    .getByRole('button', { name: `更多 ${folderRenamed}` })
    .click()
  await page.getByRole('menuitem', { name: `删除 ${folderRenamed}` }).click()
  await expect(fileRow(page, folderRenamed)).toHaveCount(0)
  await page.getByTestId('open-trash').click()
  await trashItem(page, folderRenamed)
    .getByRole('button', { name: `永久删除 ${folderRenamed}` })
    .click()
  await page.getByTestId('confirm-permanent-delete').click()
  await expect(trashItem(page, folderRenamed)).toHaveCount(0)
  await expect(page.getByTestId('usage-summary')).toContainText('已用')

  // 数据库中不得出现任何明文文件夹名称。
  const databaseUrl =
    process.env.E2E_DATABASE_URL ??
    'postgres://backup:backup-dev-password@127.0.0.1:55432/backup'
  const client = new Client({ connectionString: databaseUrl })
  await client.connect()
  try {
    const user = await client.query<{ id: string }>(
      'SELECT id FROM users WHERE email = $1::citext',
      [email],
    )
    expect(user.rowCount).toBe(1)
    const userId = user.rows[0]?.id
    expect(userId).toBeTruthy()
    if (!userId) {
      throw new Error('找不到 E2E 用户')
    }
    const columns = await client.query<{ column_name: string }>(
      `SELECT column_name
       FROM information_schema.columns
       WHERE table_schema = 'public'
         AND table_name = 'nodes'
         AND data_type IN ('bytea', 'text', 'character varying', 'character')
       ORDER BY ordinal_position`,
    )
    const names = [folderA, folderB, folderC, folderRenamed]
    for (const column of columns.rows) {
      const values = await client.query<{ value: unknown }>(
        `SELECT "${column.column_name.replaceAll('"', '""')}" AS value
         FROM nodes WHERE user_id = $1`,
        [userId],
      )
      for (const row of values.rows) {
        for (const name of names) {
          if (Buffer.isBuffer(row.value)) {
            expect(
              row.value.includes(Buffer.from(name, 'utf8')),
              `nodes.${column.column_name} 出现了明文名称 ${name}`,
            ).toBe(false)
          } else if (typeof row.value === 'string') {
            expect(
              row.value.includes(name),
              `nodes.${column.column_name} 出现了明文名称 ${name}`,
            ).toBe(false)
          }
        }
      }
    }
  } finally {
    await client.end()
  }

  const replyCount = await page.evaluate(
    () =>
      (window as unknown as { __workerReplies: unknown[] }).__workerReplies
        .length,
  )
  expect(replyCount).toBeGreaterThan(0)
  const keyLeaks = await page.evaluate(() => {
    const probe = window as unknown as {
      __workerReplies: unknown[]
      __findKeyLeaks: (roots: unknown[]) => string[]
    }
    return probe.__findKeyLeaks(probe.__workerReplies)
  })
  expect(keyLeaks).toEqual([])
  const cspViolations = await page.evaluate(
    () =>
      (window as unknown as { __cspViolations: string[] }).__cspViolations,
  )
  expect(cspViolations).toEqual([])
})
