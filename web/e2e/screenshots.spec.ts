import { randomUUID } from 'node:crypto'
import { mkdir } from 'node:fs/promises'
import path from 'node:path'

import { expect, test, type Page } from '@playwright/test'

import {
  createFolder,
  fileRow,
  registerVerifyAndLogin,
} from './session'

/**
 * design D9：人工截图审查，不做像素断言。
 * 只在 E2E_SCREENSHOTS=1 时运行，输出到 web/e2e-tmp/screenshots/。
 */
test.skip(
  process.env.E2E_SCREENSHOTS !== '1',
  '设置 E2E_SCREENSHOTS=1 才会生成截图',
)

const OUTPUT_DIR = path.resolve(process.cwd(), 'e2e-tmp', 'screenshots')

async function capture(page: Page, name: string): Promise<void> {
  await page.screenshot({ path: path.join(OUTPUT_DIR, `${name}.png`) })
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
  // 手机端 H1 视觉隐藏、标题在顶部栏，因此只断言面包屑已回到根。
  await expect(page.getByTestId('breadcrumb')).toContainText('全部文件')
}

async function openFileMenu(page: Page, name: string): Promise<void> {
  await fileRow(page, name)
    .getByRole('button', { name: `更多 ${name}` })
    .click()
}

/**
 * 按住对象 PUT，直到截图完成后再放行；不再依赖固定 sleep。
 * 返回释放函数。
 */
/** 按住目录请求以截取骨架屏，返回释放函数。 */
async function holdListRequests(page: Page): Promise<() => void> {
  let released = false
  let release: () => void = () => undefined
  const gate = new Promise<void>((resolve) => {
    release = () => {
      released = true
      resolve()
    }
  })
  await page.route('**/api/v1/nodes/**', async (route) => {
    if (route.request().method() === 'GET' && !released) {
      await gate
    }
    await route.continue()
  })
  return release
}

async function startSlowUpload(
  page: Page,
  name: string,
): Promise<() => void> {
  let released = false
  let release: () => void = () => undefined
  const gate = new Promise<void>((resolve) => {
    release = () => {
      released = true
      resolve()
    }
  })
  await page.route('**/backup/v1/**', async (route) => {
    if (!released) {
      await gate
    }
    await route.continue()
  })
  await page.setInputFiles('[data-testid="upload-input"]', {
    name,
    mimeType: 'application/octet-stream',
    buffer: Buffer.alloc(2 * 1024 * 1024, 0x42),
  })
  const item = page
    .locator(`[data-testid="transfer-item"][data-name="${name}"]`)
    .last()
  await expect(item).toHaveAttribute('data-status', 'uploading', {
    timeout: 30_000,
  })
  return release
}

async function finishUpload(page: Page, name: string): Promise<void> {
  const item = page
    .locator(`[data-testid="transfer-item"][data-name="${name}"]`)
    .last()
  await expect(item).toHaveAttribute('data-status', 'completed', {
    timeout: 120_000,
  })
  await page.unroute('**/backup/v1/**')
}

async function clearCompletedTransfers(page: Page): Promise<void> {
  const clear = page.getByTestId('transfer-clear-completed')
  if (await clear.isVisible().catch(() => false)) {
    await clear.click()
  }
}

test('桌面与手机两种视口截图', async ({ page }) => {
  // 截图需要注册、两种视口、多次上传与对话框，单独放宽超时。
  test.setTimeout(600_000)
  await mkdir(OUTPUT_DIR, { recursive: true })

  const email = `shots-${Date.now()}-${randomUUID()}@example.com`
  const emptyDesktop = `空文件夹-${randomUUID()}`
  const photoFolder = `照片-${randomUUID()}`
  const emptyMobile = `手机空文件夹-${randomUUID()}`
  const fileWithVersions = `合同-${randomUUID()}.pdf`
  const transferDesktop = `传输中-${randomUUID()}.bin`
  const transferMobile = `手机传输-${randomUUID()}.bin`

  // 桌面登录 / 注册。
  await page.setViewportSize({ width: 1440, height: 900 })
  await page.goto('/login')
  await expect(
    page.getByRole('heading', { name: '登录' }),
  ).toBeVisible()
  await capture(page, 'login-desktop')

  await page.goto('/register')
  await expect(
    page.getByRole('heading', { name: '创建账号' }),
  ).toBeVisible()
  await capture(page, 'register-desktop')

  // 手机登录 / 注册。
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/login')
  await capture(page, 'login-mobile')
  await page.goto('/register')
  await capture(page, 'register-mobile')

  // 注册并登录（桌面视口），先截空文件夹。
  await page.setViewportSize({ width: 1440, height: 900 })
  await registerVerifyAndLogin(page, email)

  // 骨架屏状态（7.5）。
  const releaseList = await holdListRequests(page)
  await page.getByTestId('refresh-files').click()
  await expect(page.getByTestId('file-skeleton')).toBeVisible()
  await capture(page, 'skeleton-desktop')
  releaseList()
  await expect(page.getByText('这个文件夹还是空的')).toBeVisible()
  await page.unroute('**/api/v1/nodes/**')

  await createFolder(page, emptyDesktop)
  await openFolder(page, emptyDesktop)
  await capture(page, 'empty-folder-desktop')
  await backToRoot(page)

  // 含数据的列表与网格。
  await createFolder(page, photoFolder)
  await page.setInputFiles('[data-testid="upload-input"]', {
    name: fileWithVersions,
    mimeType: 'application/pdf',
    buffer: Buffer.from('%PDF-1.4 screenshot'),
  })
  await finishUpload(page, fileWithVersions)
  await clearCompletedTransfers(page)
  await capture(page, 'files-list-desktop')

  await page.getByTestId('view-grid').click()
  await expect(page.getByTestId('file-grid')).toBeVisible()
  await capture(page, 'files-grid-desktop')

  // 版本历史对话框。
  await openFileMenu(page, fileWithVersions)
  await page.getByRole('menuitem', { name: `版本历史 ${fileWithVersions}` }).click()
  await expect(
    page.getByRole('dialog', { name: '版本历史' }),
  ).toBeVisible()
  await capture(page, 'versions-dialog-desktop')
  await page
    .getByRole('dialog', { name: '版本历史' })
    .getByRole('button', { name: '关闭' })
    .click()

  // 传输面板（按住对象 PUT 以截取上传中状态）。
  const releaseDesktop = await startSlowUpload(page, transferDesktop)
  await expect(page.getByTestId('transfer-panel')).toBeVisible()
  await capture(page, 'transfer-panel-desktop')
  releaseDesktop()
  await finishUpload(page, transferDesktop)
  await clearCompletedTransfers(page)

  // 回收站。
  await openFileMenu(page, photoFolder)
  await page.getByRole('menuitem', { name: `删除 ${photoFolder}` }).click()
  await page.getByTestId('open-trash').click()
  await expect(page.getByTestId('trash-view')).toBeVisible()
  await expect(page.getByTestId('trash-item')).toHaveCount(1)
  await capture(page, 'trash-desktop')

  // 账号设置。
  await page.getByRole('link', { name: '全部文件' }).click()
  await page.getByRole('button', { name: `账号菜单 ${email}` }).click()
  await page.getByRole('menuitem', { name: '账号设置' }).click()
  await expect(page.getByTestId('settings-page')).toBeVisible()
  await capture(page, 'settings-desktop')

  // 手机视口：列表、网格、抽屉、空文件夹、传输、版本、回收站、设置。
  await page.getByRole('link', { name: '全部文件' }).click()
  await page.setViewportSize({ width: 390, height: 844 })
  await page.getByTestId('view-list').click()
  await expect(page.getByTestId('file-list')).toBeVisible()
  await capture(page, 'files-list-mobile')

  await page.getByTestId('view-grid').click()
  await capture(page, 'files-grid-mobile')

  await page.getByRole('button', { name: '打开导航菜单' }).click()
  const drawer = page.getByRole('dialog', { name: '导航菜单' })
  await expect(drawer).toBeVisible()
  await capture(page, 'drawer-mobile')
  await drawer.getByRole('button', { name: '关闭导航菜单' }).click()

  // 手机空文件夹（从抽屉里的上传菜单新建）。
  await page.getByRole('button', { name: '打开导航菜单' }).click()
  await drawer.getByTestId('upload-menu').click()
  await page.getByTestId('new-folder').click()
  await page.getByTestId('folder-name').fill(emptyMobile)
  await page.getByTestId('create-folder-submit').click()
  await expect(fileRow(page, emptyMobile)).toBeVisible()
  await openFolder(page, emptyMobile)
  await capture(page, 'empty-folder-mobile')
  await backToRoot(page)

  // 手机传输面板。
  const releaseMobile = await startSlowUpload(page, transferMobile)
  await expect(page.getByTestId('transfer-panel')).toBeVisible()
  await capture(page, 'transfer-panel-mobile')
  releaseMobile()
  await finishUpload(page, transferMobile)
  await clearCompletedTransfers(page)

  // 手机版本历史。
  await openFileMenu(page, fileWithVersions)
  await page.getByRole('menuitem', { name: `版本历史 ${fileWithVersions}` }).click()
  await expect(
    page.getByRole('dialog', { name: '版本历史' }),
  ).toBeVisible()
  await capture(page, 'versions-dialog-mobile')
  await page
    .getByRole('dialog', { name: '版本历史' })
    .getByRole('button', { name: '关闭' })
    .click()

  // 手机回收站。
  await page.getByRole('button', { name: '打开导航菜单' }).click()
  await drawer.getByTestId('open-trash').click()
  await expect(page.getByTestId('trash-view')).toBeVisible()
  await capture(page, 'trash-mobile')

  // 手机账号设置。
  await page.getByRole('button', { name: `账号菜单 ${email}` }).click()
  await page.getByRole('menuitem', { name: '账号设置' }).click()
  await expect(page.getByTestId('settings-page')).toBeVisible()
  await capture(page, 'settings-mobile')
})
