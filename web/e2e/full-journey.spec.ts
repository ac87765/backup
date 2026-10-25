import { createHash, randomUUID } from 'node:crypto'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import path from 'node:path'

import { expect, test, type Page } from '@playwright/test'

import { waitForVerificationToken } from './mail'

/**
 * 容器化全链路验收（task 9.5）。
 *
 * 针对 deploy/docker-compose.e2e.yml 启动的 PostgreSQL + MinIO + 9.1 镜像，
 * 覆盖：注册 → 验证 → 登录 → 文件夹 → 上传 → 下载比对 → 新版本/恢复 →
 * 回收站 → 改密 → 恢复密钥重置 → 再次登录并下载。
 *
 * 同时把测试中出现的敏感值写入 manifest，供 deploy/scan-logs.mjs 扫描 server 日志。
 * 普通本地 E2E 也可以运行（设置 E2E_MAIL_DIR）；容器模式设置
 * E2E_MAIL_CONTAINER_SERVICE=server 与 E2E_COMPOSE_FILE。
 */

const PASSWORD = 'journey correct horse battery staple'
const NEW_PASSWORD = 'journey new correct horse battery staple'
const RESET_PASSWORD = 'journey reset correct horse battery staple'

type SensitiveManifest = {
  generatedAt: string
  literals: string[]
  presignedUrls: string[]
}

const literals = new Set<string>()
const presignedUrls = new Set<string>()
let writeChain: Promise<void> = Promise.resolve()

function record(value: string | undefined | null): void {
  if (value && value.length >= 8) {
    literals.add(value)
  }
}

function manifestPath(): string {
  return (
    process.env.E2E_SENSITIVE_VALUES ??
    path.resolve(process.cwd(), 'e2e-tmp', 'container-sensitive.json')
  )
}

async function writeSensitiveManifest(): Promise<void> {
  const manifest: SensitiveManifest = {
    generatedAt: new Date().toISOString(),
    literals: [...literals].sort(),
    presignedUrls: [...presignedUrls].sort(),
  }
  const target = manifestPath()
  await mkdir(path.dirname(target), { recursive: true })
  await writeFile(target, `${JSON.stringify(manifest, null, 2)}\n`, 'utf8')
}

function scheduleManifestWrite(): void {
  writeChain = writeChain.then(writeSensitiveManifest).catch(() => undefined)
}

function sha256(bytes: Buffer): string {
  return createHash('sha256').update(bytes).digest('hex')
}

function fileRow(page: Page, name: string) {
  return page.locator(`[data-testid="file-row"][data-name="${name}"]`)
}

function trashItem(page: Page, name: string) {
  return page.locator(`[data-testid="trash-item"][data-name="${name}"]`)
}

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

async function createFolder(page: Page, name: string): Promise<void> {
  await page.getByTestId('new-folder').click()
  await page.getByTestId('folder-name').fill(name)
  await page.getByTestId('create-folder-submit').click()
  await expect(page.getByTestId('folder-name')).toHaveCount(0)
  await expect(fileRow(page, name)).toBeVisible()
}

async function uploadFile(
  page: Page,
  name: string,
  buffer: Buffer,
  selector = '[data-testid="upload-input"]',
): Promise<void> {
  await page.setInputFiles(selector, {
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

async function downloadCurrentFile(page: Page, name: string): Promise<Buffer> {
  const downloadPromise = page.waitForEvent('download')
  await fileRow(page, name)
    .getByRole('button', { name: `下载 ${name}` })
    .click()
  const download = await downloadPromise
  const target = await download.path()
  expect(target).toBeTruthy()
  return readFile(target)
}

async function captureSessionCookie(page: Page): Promise<void> {
  const cookies = await page.context().cookies()
  for (const cookie of cookies) {
    // 会话令牌是 HttpOnly Cookie 的值；只记录长度足够的测试期值。
    if (cookie.value.length >= 16) {
      record(cookie.value)
    }
  }
  scheduleManifestWrite()
}

test.skip(
  process.env.E2E_CONTAINERIZED !== '1' &&
    !process.env.E2E_MAIL_CONTAINER_SERVICE,
  '全链路验收只针对 deploy/docker-compose.e2e.yml 的容器化服务运行',
)

test('容器化全链路：注册 → 验证 → 登录 → 文件夹 → 上传下载 → 版本 → 回收站 → 改密 → 恢复重置', async ({
  page,
  context,
}) => {
  // Chromium 没有 File System Access API 时走 Blob 下载，便于断言 SHA-256。
  await page.addInitScript(() => {
    ;(
      window as unknown as { __BACKUP_FORCE_DOWNLOAD_FALLBACK__?: string }
    ).__BACKUP_FORCE_DOWNLOAD_FALLBACK__ = 'blob'
  })

  const email = `journey-${Date.now()}-${randomUUID()}@example.com`
  const folderName = `旅程-${randomUUID()}`
  const fileName = `旅程文件-${randomUUID()}.bin`
  const contentMarker = `JOURNEY-PLAINTEXT-${randomUUID()}`
  const newContentMarker = `JOURNEY-NEW-${randomUUID()}`
  const oldContent = Buffer.concat([
    Buffer.from(contentMarker, 'utf8'),
    Buffer.alloc(5 * 1024 * 1024 + 123, 0x41),
  ])
  const newContent = Buffer.concat([
    Buffer.from(newContentMarker, 'utf8'),
    Buffer.alloc(1024 * 1024, 0x42),
  ])

  // 在测试一开始就记录明文文件名/内容片段与密码，即使后续步骤失败也能扫描已产生的日志。
  record(folderName)
  record(fileName)
  record(contentMarker)
  record(newContentMarker)
  record(PASSWORD)
  record(NEW_PASSWORD)
  record(RESET_PASSWORD)
  scheduleManifestWrite()

  // 用 context 级监听，确保 Web Worker / Service Worker 发出的预签名请求也能被捕获。
  context.on('request', (request) => {
    let url: URL
    try {
      url = new URL(request.url())
    } catch {
      return
    }
    const signatureNames = new Set(
      [
        'X-Amz-Signature',
        'X-Amz-Credential',
        'X-Amz-Security-Token',
        'Signature',
        'OSSAccessKeyId',
        'x-oss-signature',
        'x-oss-credential',
        'x-oss-security-token',
      ].map((name) => name.toLowerCase()),
    )
    let matched = false
    // OSS 的查询参数大小写不固定（x-oss-signature / OSSAccessKeyId 等），统一小写匹配。
    for (const [name, value] of url.searchParams) {
      if (signatureNames.has(name.toLowerCase()) && value) {
        record(value)
        matched = true
      }
    }
    if (matched) {
      presignedUrls.add(`${url.origin}${url.pathname}`)
      scheduleManifestWrite()
    }
  })

  try {
    // 1. 注册（恢复密钥展示 + 回填确认）。
    await page.goto('/register')
    await page.getByTestId('register-email').fill(email)
    await page.getByTestId('register-password').fill(PASSWORD)
    await page.getByTestId('register-submit').click()
    await expect(page.getByTestId('recovery-display')).toBeVisible()
    const recoveryDisplay = await page
      .getByTestId('recovery-display')
      .textContent()
    expect(recoveryDisplay).toBeTruthy()
    record(recoveryDisplay)
    await fillRecoveryConfirmation(page)
    await page.getByRole('button', { name: '确认已保存并注册' }).click()
    await expect(
      page.getByRole('heading', { name: '注册成功' }),
    ).toBeVisible()

    // 2. 从真实 file 模式邮件读取一次性令牌并验证；令牌只出现在 URL 片段。
    const mailDir = process.env.E2E_MAIL_DIR ?? ''
    const token = await waitForVerificationToken(mailDir, email)
    record(token)
    scheduleManifestWrite()
    await page.goto(`/#token=${token}`)
    await expect(
      page.getByRole('heading', { name: '邮箱已验证' }),
    ).toBeVisible()
    await expect(page).not.toHaveURL(/#token=/)

    // 3. 登录。
    await page.goto('/login')
    await page.getByTestId('login-email').fill(email)
    await page.getByTestId('login-password').fill(PASSWORD)
    await page.getByTestId('login-submit').click()
    await expect(page.getByTestId('home-email')).toHaveText(email)
    await captureSessionCookie(page)

    // 4. 文件夹 + 上传 + 下载 SHA-256 比对。
    await page.getByRole('link', { name: '文件管理' }).click()
    await expect(page.getByRole('heading', { name: '文件管理' })).toBeVisible()
    await createFolder(page, folderName)
    await uploadFile(page, fileName, oldContent)
    const downloaded = await downloadCurrentFile(page, fileName)
    expect(sha256(downloaded)).toBe(sha256(oldContent))
    scheduleManifestWrite()

    // 5. 上传新版本 → 版本历史 → 恢复旧版本 → 下载旧内容。
    await fileRow(page, fileName)
      .getByRole('button', { name: `上传新版本 ${fileName}` })
      .click()
    await page.setInputFiles('[data-testid="version-input"]', {
      name: fileName,
      mimeType: 'application/octet-stream',
      buffer: newContent,
    })
    const versionItem = page.getByTestId('transfer-item').last()
    await expect(versionItem).toHaveAttribute('data-status', 'completed', {
      timeout: 120_000,
    })
    await fileRow(page, fileName)
      .getByRole('button', { name: `版本历史 ${fileName}` })
      .click()
    const restoreButtons = page.locator('[data-testid^="restore-version-"]')
    await expect(restoreButtons).toHaveCount(1)
    await restoreButtons.last().click()
    await expect(page.getByText('已恢复该版本为当前版本')).toBeVisible()
    const downloadPromise = page.waitForEvent('download')
    await page.getByTestId('download-version-0').click()
    const versionDownload = await downloadPromise
    const versionPath = await versionDownload.path()
    expect(versionPath).toBeTruthy()
    expect(sha256(await readFile(versionPath))).toBe(
      sha256(oldContent),
    )
    const versionDialog = page.getByRole('dialog', { name: '版本历史' })
    await versionDialog.getByRole('button', { name: '关闭' }).click()
    await expect(versionDialog).toHaveCount(0)

    // 6. 回收站：删除 → 列表中可见 → 恢复。
    await fileRow(page, fileName)
      .getByRole('button', { name: `删除 ${fileName}` })
      .click()
    await expect(fileRow(page, fileName)).toHaveCount(0)
    await page.getByTestId('open-trash').click()
    await expect(trashItem(page, fileName)).toBeVisible()
    await trashItem(page, fileName)
      .getByRole('button', { name: `恢复 ${fileName}` })
      .click()
    await expect(trashItem(page, fileName)).toHaveCount(0)
    await expect(fileRow(page, fileName)).toBeVisible()

    // 7. 修改密码（旧密码证明 + 新密码），成功后仍能继续使用。
    await page.getByRole('link', { name: '返回主页' }).click()
    await page.getByRole('link', { name: '修改密码' }).click()
    await page.getByTestId('change-old-password').fill(PASSWORD)
    await page.getByTestId('change-new-password').fill(NEW_PASSWORD)
    await page.getByTestId('change-confirm-password').fill(NEW_PASSWORD)
    await page.getByTestId('change-submit').click()
    await expect(page.getByRole('status')).toContainText('密码已修改')
    await page.getByRole('link', { name: '返回主页' }).click()
    await page.getByRole('button', { name: '退出登录' }).click()
    await expect(
      page.getByRole('heading', { name: '端对端加密备份' }),
    ).toBeVisible()

    // 8. 用恢复密钥重置密码（脱离密码也能恢复访问），再用新密码登录。
    await page.goto('/reset')
    await page.getByTestId('reset-email').fill(email)
    await page.getByTestId('reset-recovery-key').fill(recoveryDisplay ?? '')
    await page.getByTestId('reset-new-password').fill(RESET_PASSWORD)
    await page.getByTestId('reset-confirm-password').fill(RESET_PASSWORD)
    await page.getByTestId('reset-submit').click()
    await expect(page.getByRole('status')).toContainText('密码已重置')

    await page.goto('/login')
    await page.getByTestId('login-email').fill(email)
    await page.getByTestId('login-password').fill(RESET_PASSWORD)
    await page.getByTestId('login-submit').click()
    await expect(page.getByTestId('home-email')).toHaveText(email)
    await captureSessionCookie(page)

    // 9. 重置后数据仍可解密：文件与文件夹都在，下载当前版本（已恢复的旧内容）比对。
    await page.getByRole('link', { name: '文件管理' }).click()
    await expect(fileRow(page, folderName)).toBeVisible()
    await expect(fileRow(page, fileName)).toBeVisible()
    const afterReset = await downloadCurrentFile(page, fileName)
    expect(sha256(afterReset)).toBe(sha256(oldContent))
  } finally {
    // 等待所有异步 manifest 写入完成，并把最新值落盘（失败时也供日志扫描使用）。
    await writeChain
    await writeSensitiveManifest()
  }
})
