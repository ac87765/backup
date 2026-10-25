import { createHash, randomUUID } from 'node:crypto'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { expect, test, type Page } from '@playwright/test'
import { Client } from 'pg'

import { waitForVerificationToken } from './mail'

const PASSWORD = 'file transfer correct horse battery staple'
const BUCKET_ROUTE = '**/backup/v1/**'

function sha256(bytes: Buffer): string {
  return createHash('sha256').update(bytes).digest('hex')
}

async function fillRecoveryConfirmation(page: Page): Promise<void> {
  const display = await page.getByTestId('recovery-display').textContent()
  expect(display).toBeTruthy()
  const groups = (display ?? '').split('-').filter(Boolean)
  const inputs = page.locator('[data-testid^="recovery-confirm-"]')
  const count = await inputs.count()
  for (let position = 0; position < count; position += 1) {
    const input = inputs.nth(position)
    const groupIndex = Number(await input.getAttribute('data-group-index'))
    await input.fill(groups[groupIndex] ?? '')
  }
}

async function registerVerifyAndLogin(page: Page, email: string): Promise<void> {
  await page.goto('/register')
  await page.getByTestId('register-email').fill(email)
  await page.getByTestId('register-password').fill(PASSWORD)
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

  await page.goto('/login')
  await page.getByTestId('login-email').fill(email)
  await page.getByTestId('login-password').fill(PASSWORD)
  await page.getByTestId('login-submit').click()
  await expect(page.getByTestId('home-email')).toHaveText(email)
  await expect(page.getByRole('heading', { name: '我的文件' })).toBeVisible()
}

async function installDownloadFallback(page: Page): Promise<void> {
  await page.addInitScript(() => {
    ;(
      window as unknown as { __BACKUP_FORCE_DOWNLOAD_FALLBACK__?: string }
    ).__BACKUP_FORCE_DOWNLOAD_FALLBACK__ = 'blob'
  })
}

async function installServiceWorkerFallback(page: Page): Promise<void> {
  await page.addInitScript(() => {
    ;(
      window as unknown as { __BACKUP_FORCE_DOWNLOAD_FALLBACK__?: string }
    ).__BACKUP_FORCE_DOWNLOAD_FALLBACK__ = 'service-worker'
  })
}

function fileRow(page: Page, name: string) {
  return page.locator(`[data-testid="file-row"][data-name="${name}"]`)
}

async function uploadFile(
  page: Page,
  name: string,
  buffer: Buffer,
): Promise<void> {
  await page.setInputFiles('[data-testid="upload-input"]', {
    name,
    mimeType: 'application/octet-stream',
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

async function downloadCurrentFile(
  page: Page,
  name: string,
): Promise<Buffer> {
  const downloadPromise = page.waitForEvent('download')
  await fileRow(page, name).getByRole('button', { name: `下载 ${name}` }).click()
  const download = await downloadPromise
  const path = await download.path()
  expect(path).toBeTruthy()
  return readFile(path)
}

function databaseUrl(): string {
  return (
    process.env.E2E_DATABASE_URL ??
    'postgres://backup:backup-dev-password@127.0.0.1:55432/backup'
  )
}

test('多块文件上传 → 下载 SHA-256 一致；数据库与对象存储不含明文', async ({
  page,
}) => {
  await installDownloadFallback(page)
  // Worker → 主线程边界探针：任何回复都不得包含 32 字节密钥。
  await page.addInitScript(() => {
    const replies: unknown[] = []
    ;(window as unknown as { __workerReplies: unknown[] }).__workerReplies =
      replies
    const NativeWorker = window.Worker
    const RecordingWorker = function (this: Worker, ...args: unknown[]) {
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

  const email = `transfer-${Date.now()}-${randomUUID()}@example.com`
  const name = `多块-${randomUUID()}.bin`
  const marker = `PLAINTEXT-MARKER-${randomUUID()}`
  const buffer = Buffer.alloc(9 * 1024 * 1024, 0x41)
  Buffer.from(marker, 'utf8').copy(buffer, 0)

  await registerVerifyAndLogin(page, email)
  await uploadFile(page, name, buffer)
  const downloaded = await downloadCurrentFile(page, name)
  expect(sha256(downloaded)).toBe(sha256(buffer))

  const client = new Client({ connectionString: databaseUrl() })
  await client.connect()
  try {
    const user = await client.query<{ id: string }>(
      'SELECT id FROM users WHERE email = $1::citext',
      [email],
    )
    const userId = user.rows[0]?.id
    expect(userId).toBeTruthy()
    const node = await client.query<{ id: string; current_version_id: string }>(
      `SELECT id, current_version_id
       FROM nodes
       WHERE user_id = $1 AND kind = 'file'
       ORDER BY created_at DESC
       LIMIT 1`,
      [userId],
    )
    const nodeId = node.rows[0]?.id
    const versionId = node.rows[0]?.current_version_id
    expect(nodeId).toBeTruthy()
    expect(versionId).toBeTruthy()

    // 1) 数据库任意 text/bytea 列都不得包含明文文件名或明文特征串。
    const columns = await client.query<{ column_name: string }>(
      `SELECT column_name
       FROM information_schema.columns
       WHERE table_schema = 'public'
         AND table_name = 'nodes'
         AND data_type IN ('bytea', 'text', 'character varying', 'character')
       ORDER BY ordinal_position`,
    )
    for (const column of columns.rows) {
      const values = await client.query<{ value: unknown }>(
        `SELECT "${column.column_name.replaceAll('"', '""')}" AS value
         FROM nodes WHERE user_id = $1`,
        [userId],
      )
      for (const row of values.rows) {
        if (Buffer.isBuffer(row.value)) {
          expect(
            row.value.includes(Buffer.from(name, 'utf8')),
            `nodes.${column.column_name} 出现了明文文件名`,
          ).toBe(false)
          expect(
            row.value.includes(Buffer.from(marker, 'utf8')),
            `nodes.${column.column_name} 出现了明文内容`,
          ).toBe(false)
        } else if (typeof row.value === 'string') {
          expect(row.value.includes(name)).toBe(false)
          expect(row.value.includes(marker)).toBe(false)
        }
      }
    }

    // 2) 对象存储中的密文不得包含明文特征串。
    const manifestResponse = await page.request.get(
      `/api/v1/nodes/${nodeId}/versions/${versionId}/download`,
    )
    expect(manifestResponse.ok()).toBeTruthy()
    const manifest = (await manifestResponse.json()) as {
      chunks: Array<{ url: string }>
    }
    const objectResponse = await fetch(manifest.chunks[0].url)
    expect(objectResponse.ok).toBeTruthy()
    const ciphertext = Buffer.from(await objectResponse.arrayBuffer())
    expect(ciphertext.includes(Buffer.from(marker, 'utf8'))).toBe(false)
  } finally {
    await client.end()
  }

  const leaks = await page.evaluate(() => {
    const replies = (window as unknown as { __workerReplies: unknown[] })
      .__workerReplies
    const found: string[] = []
    const visit = (value: unknown, path: string) => {
      if (value instanceof Uint8Array || value instanceof ArrayBuffer) {
        if (value.byteLength === 32) {
          found.push(path)
        }
        return
      }
      if (typeof value === 'string') {
        if (/^[0-9a-fA-F]{64}$/.test(value)) {
          found.push(`${path}:hex64`)
        }
        return
      }
      if (Array.isArray(value)) {
        value.forEach((nested, index) => visit(nested, `${path}[${index}]`))
        return
      }
      if (typeof value === 'object' && value !== null) {
        for (const [key, nested] of Object.entries(value)) {
          visit(nested, `${path}.${key}`)
        }
      }
    }
    replies.forEach((reply, index) => visit(reply, `message[${index}]`))
    return found
  })
  expect(leaks).toEqual([])
})

test('Service Worker 流式下载多块文件 SHA-256 一致', async ({ page }) => {
  await installServiceWorkerFallback(page)
  const email = `sw-${Date.now()}-${randomUUID()}@example.com`
  const name = `sw-${randomUUID()}.bin`
  const buffer = Buffer.alloc(4 * 1024 * 1024 + 100, 0x44)
  Buffer.from('SERVICE-WORKER-PLAINTEXT', 'utf8').copy(buffer, 0)
  await registerVerifyAndLogin(page, email)
  await uploadFile(page, name, buffer)
  const downloaded = await downloadCurrentFile(page, name)
  expect(sha256(downloaded)).toBe(sha256(buffer))
})

test('上传过程中断网再恢复，最终完成', async ({ page, context }) => {
  await installDownloadFallback(page)
  const email = `offline-${Date.now()}-${randomUUID()}@example.com`
  const name = `断网-${randomUUID()}.bin`
  const buffer = Buffer.alloc(16 * 1024 * 1024, 0x42)
  await registerVerifyAndLogin(page, email)

  await page.setInputFiles('[data-testid="upload-input"]', {
    name,
    mimeType: 'application/octet-stream',
    buffer,
  })
  await context.setOffline(true)
  await page.waitForTimeout(1_500)
  await context.setOffline(false)
  const item = page
    .locator(`[data-testid="transfer-item"][data-name="${name}"]`)
    .last()
  await expect(item).toHaveAttribute('data-status', 'completed', {
    timeout: 180_000,
  })
  await expect(fileRow(page, name)).toBeVisible()
})

test('上传新版本 → 恢复旧版本 → 下载旧内容', async ({ page }) => {
  await installDownloadFallback(page)
  const email = `version-${Date.now()}-${randomUUID()}@example.com`
  const name = `版本-${randomUUID()}.txt`
  const oldContent = Buffer.from(`OLD-CONTENT-${randomUUID()}`)
  const newContent = Buffer.from(`NEW-CONTENT-${randomUUID()}-${'x'.repeat(200)}`)
  await registerVerifyAndLogin(page, email)
  await uploadFile(page, name, oldContent)

  await fileRow(page, name)
    .getByRole('button', { name: `更多 ${name}` })
    .click()
  await page.getByRole('menuitem', { name: `上传新版本 ${name}` }).click()
  await page.setInputFiles('[data-testid="version-input"]', {
    name,
    mimeType: 'text/plain',
    buffer: newContent,
  })
  const item = page.getByTestId('transfer-item').last()
  await expect(item).toHaveAttribute('data-status', 'completed', {
    timeout: 120_000,
  })

  await fileRow(page, name)
    .getByRole('button', { name: `更多 ${name}` })
    .click()
  await page.getByRole('menuitem', { name: `版本历史 ${name}` }).click()
  const restoreButtons = page.locator('[data-testid^="restore-version-"]')
  await expect(restoreButtons).toHaveCount(1)
  await restoreButtons.last().click()
  await expect(page.getByText('已恢复该版本为当前版本')).toBeVisible()
  await expect(page.getByTestId('download-version-0')).toBeVisible()

  const downloadPromise = page.waitForEvent('download')
  await page.getByTestId('download-version-0').click()
  const download = await downloadPromise
  const path = await download.path()
  expect(path).toBeTruthy()
  const downloaded = await readFile(path)
  expect(downloaded).toEqual(oldContent)
})

test('空文件上传下载', async ({ page }) => {
  await installDownloadFallback(page)
  const email = `empty-${Date.now()}-${randomUUID()}@example.com`
  const name = `空文件-${randomUUID()}.bin`
  await registerVerifyAndLogin(page, email)
  await uploadFile(page, name, Buffer.alloc(0))
  const downloaded = await downloadCurrentFile(page, name)
  expect(sha256(downloaded)).toBe(sha256(Buffer.alloc(0)))
})

test('取消上传后配额预留释放', async ({ page }) => {
  await installDownloadFallback(page)
  const email = `cancel-${Date.now()}-${randomUUID()}@example.com`
  const name = `取消-${randomUUID()}.bin`
  await registerVerifyAndLogin(page, email)
  // 人为放慢对象 PUT，确保取消按钮在会话创建后仍有机会点击。
  await page.route(BUCKET_ROUTE, async (route) => {
    await new Promise((resolve) => setTimeout(resolve, 500))
    await route.continue()
  })

  // setInputFiles 不允许直接传 >50 MB 的 buffer；写入临时文件后传路径。
  const directory = await mkdtemp(join(tmpdir(), 'backup-e2e-cancel-'))
  const filePath = join(directory, name)
  await writeFile(filePath, Buffer.alloc(64 * 1024 * 1024, 0x43))
  try {
    await page.setInputFiles('[data-testid="upload-input"]', filePath)
    const item = page
      .locator(`[data-testid="transfer-item"][data-name="${name}"]`)
      .last()
    await expect(item).toHaveAttribute('data-status', 'uploading', {
      timeout: 60_000,
    })
    await expect(item.getByTestId('transfer-cancel')).toBeVisible()
    await item.getByTestId('transfer-cancel').click()
    await expect(item).toHaveAttribute('data-status', 'cancelled', {
      timeout: 60_000,
    })
    await expect(fileRow(page, name)).toHaveCount(0)
    await expect(page.getByTestId('usage-summary')).toContainText('预留 0 B', {
      timeout: 60_000,
    })
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})
