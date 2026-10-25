import { createHash, randomUUID } from 'node:crypto'
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'

import { expect, test, type Page } from '@playwright/test'
import { Client } from 'pg'

import {
  createFolder,
  fileRow,
  openMoreMenu,
  registerVerifyAndLogin,
} from './session'

/**
 * 文件夹备份 / 恢复（add-folder-backup）。
 *
 * - 上传：`webkitdirectory` 的 FileList 与拖拽 `webkitGetAsEntry`；
 * - 增量：同名文件作为新版本、未变化跳过、旧版本可恢复；
 * - 分组进度、取消、重试、429 自动退避；
 * - 客户端流式 zip 打包下载与篡改中止。
 */

const createdDirs: string[] = []

async function makeLocalFolder(
  rootName: string,
  files: Record<string, string>,
): Promise<string> {
  const parent = await mkdtemp(join(tmpdir(), 'backup-folder-'))
  createdDirs.push(parent)
  const root = join(parent, rootName)
  await mkdir(root, { recursive: true })
  for (const [relative, content] of Object.entries(files)) {
    const target = join(root, relative)
    await mkdir(dirname(target), { recursive: true })
    await writeFile(target, content)
  }
  return root
}

test.afterEach(async () => {
  while (createdDirs.length > 0) {
    const directory = createdDirs.pop()
    if (directory) {
      await rm(directory, { recursive: true, force: true })
    }
  }
})

async function installBlobDownload(page: Page): Promise<void> {
  await page.addInitScript(() => {
    ;(
      window as unknown as { __BACKUP_FORCE_DOWNLOAD_FALLBACK__?: string }
    ).__BACKUP_FORCE_DOWNLOAD_FALLBACK__ = 'blob'
  })
}

function folderJob(page: Page) {
  return page.locator('[data-testid="folder-job"]').last()
}

async function waitFolderDone(page: Page, timeout = 240_000): Promise<void> {
  await expect(folderJob(page)).toHaveAttribute('data-status', 'done', {
    timeout,
  })
}

async function uploadFolder(page: Page, directory: string): Promise<void> {
  await page.setInputFiles('[data-testid="folder-upload-input"]', directory)
}

async function openFolder(page: Page, name: string): Promise<void> {
  await fileRow(page, name)
    .getByRole('button', { name: `打开 ${name}` })
    .click()
  await expect(page.getByTestId('breadcrumb')).toContainText(name)
}

async function backToRoot(page: Page): Promise<void> {
  await page
    .getByTestId('breadcrumb')
    .getByRole('button', { name: '全部文件' })
    .click()
  await expect(page.getByTestId('breadcrumb')).toContainText('全部文件')
}

async function downloadFile(
  page: Page,
  name: string,
): Promise<Buffer> {
  const downloadPromise = page.waitForEvent('download')
  await fileRow(page, name)
    .getByRole('button', { name: `下载 ${name}` })
    .click()
  const download = await downloadPromise
  const path = await download.path()
  expect(path).toBeTruthy()
  return readFile(path)
}

function sha256(bytes: Buffer): string {
  return createHash('sha256').update(bytes).digest('hex')
}

/** 解析 STORE zip 的中央目录（测试用，无压缩、无 ZIP64）。 */
function readZipEntries(zip: Buffer): Array<{ name: string; data: Buffer }> {
  const eocd = zip.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]))
  expect(eocd).toBeGreaterThanOrEqual(0)
  const count = zip.readUInt16LE(eocd + 10)
  const centralOffset = zip.readUInt32LE(eocd + 16)
  const entries: Array<{ name: string; data: Buffer }> = []
  let offset = centralOffset
  for (let index = 0; index < count; index += 1) {
    expect(zip.readUInt32LE(offset)).toBe(0x02014b50)
    const method = zip.readUInt16LE(offset + 10)
    const compressedSize = zip.readUInt32LE(offset + 20)
    const nameLength = zip.readUInt16LE(offset + 28)
    const extraLength = zip.readUInt16LE(offset + 30)
    const commentLength = zip.readUInt16LE(offset + 32)
    const localOffset = zip.readUInt32LE(offset + 42)
    const name = zip.subarray(offset + 46, offset + 46 + nameLength).toString('utf8')
    expect(method, `${name} 应使用 STORE（不压缩）`).toBe(0)
    const localNameLength = zip.readUInt16LE(localOffset + 26)
    const localExtraLength = zip.readUInt16LE(localOffset + 28)
    const dataStart = localOffset + 30 + localNameLength + localExtraLength
    entries.push({
      name,
      data: zip.subarray(dataStart, dataStart + compressedSize),
    })
    offset += 46 + nameLength + extraLength + commentLength
  }
  return entries
}

/** 用伪造的 FileSystemEntry 树模拟拖入一个空文件夹。 */
async function dropEmptyFolder(page: Page, name: string): Promise<void> {
  await page.evaluate((folderName) => {
    const emptyDirectory = {
      name: folderName,
      isFile: false,
      isDirectory: true,
      createReader: () => {
        let finished = false
        return {
          readEntries: (success: (entries: unknown[]) => void) => {
            if (finished) {
              success([])
              return
            }
            finished = true
            success([])
          },
        }
      },
    }
    const item = {
      kind: 'file',
      webkitGetAsEntry: () => emptyDirectory,
    }
    const transfer = new DataTransfer()
    transfer.items.add(new File(['placeholder'], 'placeholder.txt'))
    Object.defineProperty(transfer, 'items', {
      value: { 0: item, length: 1 },
    })
    window.dispatchEvent(
      new DragEvent('drop', { bubbles: true, dataTransfer: transfer }),
    )
  }, name)
}

test('文件夹上传：子目录、拖入空文件夹、忽略系统文件、逐文件下载比对', async ({
  page,
}) => {
  await installBlobDownload(page)
  const email = `folder-${Date.now()}-${randomUUID()}@example.com`
  const rootName = `照片-${randomUUID()}`
  const a = Buffer.from(`A-${randomUUID()}`)
  const b = Buffer.from(`B-${randomUUID()}-${'x'.repeat(100)}`)
  const c = Buffer.from(`C-${randomUUID()}`)
  const directory = await makeLocalFolder(rootName, {
    '2025/a.jpg': a.toString(),
    '2025/b.jpg': b.toString(),
    '2026/c.jpg': c.toString(),
    '.DS_Store': 'junk',
    'Thumbs.db': 'junk',
  })

  await registerVerifyAndLogin(page, email)
  // 上传菜单入口存在。
  await page.getByTestId('upload-menu').click()
  await expect(page.getByTestId('upload-folder')).toBeVisible()
  await page.keyboard.press('Escape')

  await uploadFolder(page, directory)
  await waitFolderDone(page)
  await expect(fileRow(page, rootName)).toBeVisible()

  await openFolder(page, rootName)
  await expect(fileRow(page, '2025')).toBeVisible()
  await expect(fileRow(page, '2026')).toBeVisible()
  await expect(fileRow(page, '.DS_Store')).toHaveCount(0)
  await expect(fileRow(page, 'Thumbs.db')).toHaveCount(0)

  await openFolder(page, '2025')
  expect((await downloadFile(page, 'a.jpg')).equals(a)).toBe(true)
  expect((await downloadFile(page, 'b.jpg')).equals(b)).toBe(true)

  // 拖入一个空文件夹：云端创建空目录。
  await backToRoot(page)
  const emptyName = `草稿-${randomUUID()}`
  await dropEmptyFolder(page, emptyName)
  await waitFolderDone(page)
  await expect(fileRow(page, emptyName)).toBeVisible()
})

test('第二次备份合并同名文件夹、只更新变化文件、旧版本可恢复', async ({
  page,
}) => {
  await installBlobDownload(page)
  const email = `incremental-${Date.now()}-${randomUUID()}@example.com`
  const rootName = `工作资料-${randomUUID()}`
  const original = Buffer.from('ORIGINAL-CONTENT-0123456789')
  const updated = Buffer.from('UPDATED_-CONTENT-0123456789')
  const files: Record<string, string> = {}
  for (let index = 0; index < 10; index += 1) {
    files[`file-${index}.txt`] =
      index === 0 ? original.toString() : `UNCHANGED-${index}`
  }
  const directory = await makeLocalFolder(rootName, files)

  await registerVerifyAndLogin(page, email)
  await uploadFolder(page, directory)
  await waitFolderDone(page)

  // 修改 1 个（大小相同，走 SHA-256 判定）、新增 1 个，其余 9 个不变。
  await writeFile(join(directory, 'file-0.txt'), updated)
  await writeFile(join(directory, 'new.txt'), 'BRAND-NEW-FILE')
  await uploadFolder(page, directory)
  await waitFolderDone(page)
  const job = folderJob(page)
  await expect(job).toContainText('新增 1 · 更新 1 · 未变化 9')

  // 当前版本是更新后的内容。
  await openFolder(page, rootName)
  expect((await downloadFile(page, 'file-0.txt')).equals(updated)).toBe(true)

  // 版本历史中有第一次备份的版本，恢复后下载得到旧内容。
  await openMoreMenu(page, 'file-0.txt')
  await page
    .getByRole('menuitem', { name: '版本历史 file-0.txt' })
    .click()
  const restoreButtons = page.locator('[data-testid^="restore-version-"]')
  await expect(restoreButtons.first()).toBeVisible()
  await restoreButtons.last().click()
  await expect(page.getByText('已恢复该版本为当前版本')).toBeVisible()
  await page
    .getByRole('dialog', { name: '版本历史' })
    .getByRole('button', { name: '关闭' })
    .click()
  expect((await downloadFile(page, 'file-0.txt')).equals(original)).toBe(true)

  // 同一次备份新增的文件也在。
  await expect(fileRow(page, 'new.txt')).toBeVisible()
  await expect(fileRow(page, 'file-9.txt')).toBeVisible()
})

test('配额不足时预检失败，不创建任何文件夹', async ({ page }) => {
  const email = `quota-${Date.now()}-${randomUUID()}@example.com`
  await page.route('**/api/v1/me/usage', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        used_bytes: 0,
        reserved_bytes: 0,
        quota_bytes: 10,
      }),
    })
  })
  const rootName = `配额-${randomUUID()}`
  const directory = await makeLocalFolder(rootName, {
    'big.bin': 'x'.repeat(1024),
  })
  await registerVerifyAndLogin(page, email)
  await uploadFolder(page, directory)
  await expect(page.getByText(/空间不足：需要/).first()).toBeVisible({
    timeout: 30_000,
  })
  await expect(page.locator(`[data-testid="file-row"][data-name="${rootName}"]`)).toHaveCount(0)
})

test('取消文件夹任务：进行中的上传中止且配额预留释放', async ({ page }) => {
  const email = `cancel-folder-${Date.now()}-${randomUUID()}@example.com`
  const rootName = `取消-${randomUUID()}`
  const directory = await makeLocalFolder(rootName, {
    'a.bin': 'A'.repeat(2 * 1024 * 1024),
    'b.bin': 'B'.repeat(2 * 1024 * 1024),
  })
  await page.route('**/backup/v1/**', async (route) => {
    await new Promise((resolve) => setTimeout(resolve, 5_000))
    await route.continue()
  })
  await registerVerifyAndLogin(page, email)
  await uploadFolder(page, directory)
  await expect(folderJob(page)).toHaveAttribute('data-status', 'running', {
    timeout: 30_000,
  })
  await page.getByTestId('folder-job-cancel').click()
  await expect(folderJob(page)).toHaveAttribute('data-status', 'cancelled', {
    timeout: 30_000,
  })
  await page.unroute('**/backup/v1/**')
  await expect(page.getByTestId('usage-summary')).not.toContainText('预留', {
    timeout: 60_000,
  })
})

test('失败项一键重试：只重传失败文件，失败数归零', async ({ page }) => {
  const email = `retry-folder-${Date.now()}-${randomUUID()}@example.com`
  const rootName = `重试-${randomUUID()}`
  const directory = await makeLocalFolder(rootName, {
    'a.txt': 'A',
    'b.txt': 'B',
  })
  let rejecting = true
  await page.route('**/backup/v1/**', async (route) => {
    if (rejecting && route.request().method() === 'PUT') {
      await route.fulfill({ status: 400, body: 'rejected by test' })
      return
    }
    await route.continue()
  })
  await registerVerifyAndLogin(page, email)
  await uploadFolder(page, directory)
  await expect(folderJob(page)).toHaveAttribute('data-status', 'failed', {
    timeout: 60_000,
  })
  rejecting = false
  await page.getByTestId('folder-job-retry').click()
  await expect(folderJob(page)).toHaveAttribute('data-status', 'done', {
    timeout: 120_000,
  })
  await expect(folderJob(page)).toContainText('失败 0')
})

test('文件夹下载为 zip：结构、空文件夹、STORE 与 SHA-256 一致', async ({
  page,
}) => {
  await installBlobDownload(page)
  const email = `zip-${Date.now()}-${randomUUID()}@example.com`
  const rootName = `照片-${randomUUID()}`
  const a = Buffer.from(`ZIP-A-${randomUUID()}`)
  const b = Buffer.from(`ZIP-B-${randomUUID()}-${'y'.repeat(200)}`)
  const directory = await makeLocalFolder(rootName, {
    '2025/a.txt': a.toString(),
    'b.txt': b.toString(),
  })
  await registerVerifyAndLogin(page, email)
  await uploadFolder(page, directory)
  await waitFolderDone(page)

  // 在云端文件夹内创建一个空目录。
  await openFolder(page, rootName)
  await createFolder(page, '草稿')
  await backToRoot(page)

  const downloadPromise = page.waitForEvent('download')
  await openMoreMenu(page, rootName)
  await page
    .getByRole('menuitem', { name: `下载为 .zip ${rootName}` })
    .click()
  await expect(
    page.locator('[data-testid="folder-job"][data-direction="download"]'),
  ).toHaveCount(1)
  const download = await downloadPromise
  expect(download.suggestedFilename()).toBe(`${rootName}.zip`)
  const zipPath = await download.path()
  expect(zipPath).toBeTruthy()
  const zip = await readFile(zipPath)
  const entries = readZipEntries(zip)
  const names = entries.map((entry) => entry.name)
  expect(names).toEqual(
    expect.arrayContaining(['2025/a.txt', 'b.txt', '草稿/']),
  )
  const entryA = entries.find((entry) => entry.name === '2025/a.txt')
  const entryB = entries.find((entry) => entry.name === 'b.txt')
  expect(sha256(entryA?.data ?? Buffer.alloc(0))).toBe(sha256(a))
  expect(sha256(entryB?.data ?? Buffer.alloc(0))).toBe(sha256(b))
  const empty = entries.find((entry) => entry.name === '草稿/')
  expect(empty?.data.byteLength ?? -1).toBe(0)

  // 根目录整体下载文件名为“加密备份.zip”。
  const rootDownloadPromise = page.waitForEvent('download')
  await page.getByTestId('download-folder-zip').click()
  const rootDownload = await rootDownloadPromise
  expect(rootDownload.suggestedFilename()).toBe('加密备份.zip')
})

test('打包中密文被篡改：整体中止，不产出完整 zip', async ({ page }) => {
  await installBlobDownload(page)
  const email = `zip-tamper-${Date.now()}-${randomUUID()}@example.com`
  const rootName = `篡改-${randomUUID()}`
  const directory = await makeLocalFolder(rootName, {
    'a.txt': 'TAMPER-ME',
  })
  await registerVerifyAndLogin(page, email)
  await uploadFolder(page, directory)
  await waitFolderDone(page)

  await page.route('**/backup/v1/**', async (route) => {
    if (route.request().method() !== 'GET') {
      await route.continue()
      return
    }
    const response = await route.fetch()
    const body = Buffer.from(await response.body())
    if (body.length > 0) {
      body[body.length - 1] = (body[body.length - 1] ?? 0) ^ 0xff
    }
    await route.fulfill({ response, body })
  })

  let downloaded = false
  page.on('download', () => {
    downloaded = true
  })
  await openMoreMenu(page, rootName)
  await page
    .getByRole('menuitem', { name: `下载为 .zip ${rootName}` })
    .click()
  const downloadJob = page.locator(
    '[data-testid="folder-job"][data-direction="download"]',
  )
  await expect(downloadJob).toHaveAttribute('data-status', 'failed', {
    timeout: 60_000,
  })
  await expect(downloadJob).toContainText(/完整性|块解密|失败/)
  expect(downloaded).toBe(false)
  await page.unroute('**/backup/v1/**')
})

test('明文 SHA-256 只存在于加密元数据，不进入数据库与对象存储', async ({
  page,
}) => {
  const email = `sha-${Date.now()}-${randomUUID()}@example.com`
  const rootName = `摘要-${randomUUID()}`
  const content = Buffer.from(`PLAINTEXT-${randomUUID()}`)
  const digest = createHash('sha256').update(content).digest()
  const hex = digest.toString('hex')
  const directory = await makeLocalFolder(rootName, {
    'a.txt': content.toString(),
  })
  await registerVerifyAndLogin(page, email)
  await uploadFolder(page, directory)
  await waitFolderDone(page)

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
    const userId = user.rows[0]?.id
    expect(userId).toBeTruthy()
    const node = await client.query<{
      id: string
      current_version_id: string
    }>(
      `SELECT id, current_version_id FROM nodes
       WHERE user_id = $1 AND kind = 'file'
       ORDER BY created_at DESC LIMIT 1`,
      [userId],
    )
    const nodeId = node.rows[0]?.id ?? ''
    const versionId = node.rows[0]?.current_version_id ?? ''
    expect(nodeId).toBeTruthy()
    expect(versionId).toBeTruthy()

    for (const table of ['nodes', 'file_versions']) {
      const columns = await client.query<{ column_name: string }>(
        `SELECT column_name FROM information_schema.columns
         WHERE table_schema = 'public' AND table_name = $1
           AND data_type IN ('bytea','text','character varying','character')
         ORDER BY ordinal_position`,
        [table],
      )
      for (const column of columns.rows) {
        const values = await client.query<{ value: unknown }>(
          `SELECT "${column.column_name.replaceAll('"', '""')}" AS value FROM ${table}`,
        )
        for (const row of values.rows) {
          if (Buffer.isBuffer(row.value)) {
            expect(row.value.includes(Buffer.from(hex))).toBe(false)
            expect(row.value.includes(digest)).toBe(false)
          } else if (typeof row.value === 'string') {
            expect(row.value.includes(hex)).toBe(false)
          }
        }
      }
    }

    // 对象存储中的密文同样不包含摘要的十六进制或原始字节。
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
    expect(ciphertext.includes(Buffer.from(hex))).toBe(false)
    expect(ciphertext.includes(digest)).toBe(false)
  } finally {
    await client.end()
  }
})

test('1000 个小文件在低限流配置下全部完成', async ({ page }) => {
  test.skip(
    process.env.E2E_FOLDER_RATE_LIMIT_TEST !== '1',
    '需要以 E2E_FOLDER_RATE_LIMIT_TEST=1 与较低的 E2E_USER_WRITE_RATE_LIMIT 启动',
  )
  test.setTimeout(900_000)
  const email = `rate-${Date.now()}-${randomUUID()}@example.com`
  const rootName = `千文件-${randomUUID()}`
  const files: Record<string, string> = {}
  for (let index = 0; index < 1000; index += 1) {
    files[`f-${String(index).padStart(4, '0')}.txt`] = `file-${index}`
  }
  const directory = await makeLocalFolder(rootName, files)
  const rateLimited: number[] = []
  page.on('response', (response) => {
    if (
      response.url().includes('/api/v1/') &&
      response.status() === 429
    ) {
      rateLimited.push(429)
    }
  })

  await registerVerifyAndLogin(page, email)
  await uploadFolder(page, directory)
  await waitFolderDone(page, 900_000)
  const job = folderJob(page)
  await expect(job).toHaveAttribute('data-total', '1000')
  await expect(job).toHaveAttribute('data-done', '1000')
  await expect(job).toContainText('失败 0')
  expect(rateLimited.length).toBeGreaterThan(0)
})
