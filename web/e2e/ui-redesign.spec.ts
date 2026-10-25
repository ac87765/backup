import { randomUUID } from 'node:crypto'

import { expect, test } from '@playwright/test'

import {
  createFolder,
  fileRow,
  registerVerifyAndLogin,
  unlockWithPassword,
} from './session'

test('右键菜单、网格持久化、本地筛选、旧路由与 localStorage', async ({
  page,
}) => {
  const email = `ui-${Date.now()}-${randomUUID()}@example.com`
  const prefix = `界面-${randomUUID()}`
  const folderA = `${prefix}-甲`
  const folderB = `${prefix}-乙`
  const folderRenamed = `${prefix}-甲2`

  await registerVerifyAndLogin(page, email)
  await expect(page.getByRole('heading', { name: '全部文件' })).toBeVisible()
  await expect(
    page.getByRole('link', { name: '全部文件' }),
  ).toHaveAttribute('aria-current', 'page')

  await createFolder(page, folderA)
  await createFolder(page, folderB)

  // 右键菜单与“更多”菜单相同；Esc 关闭。
  await fileRow(page, folderA).click({ button: 'right' })
  await expect(
    page.getByRole('menuitem', { name: `重命名 ${folderA}` }),
  ).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.getByRole('menu')).toHaveCount(0)

  await fileRow(page, folderA).click({ button: 'right' })
  await page.getByRole('menuitem', { name: `重命名 ${folderA}` }).click()
  await expect(page.getByTestId('rename-name')).toHaveValue(folderA)
  await page.getByTestId('rename-name').fill(folderRenamed)
  await page.getByTestId('rename-submit').click()
  await expect(fileRow(page, folderRenamed)).toBeVisible()

  // 排序 + 网格视图写入本地偏好，刷新并解锁后保持网格。
  await page.getByRole('button', { name: '大小' }).click()
  await page.getByTestId('view-grid').click()
  await expect(page.getByTestId('file-grid')).toBeVisible()
  await page.reload()
  await unlockWithPassword(page)
  await expect(page.getByTestId('file-grid')).toBeVisible()

  // 当前文件夹筛选只在本地过滤，不产生新的列表请求。
  let nodeRequests = 0
  page.on('request', (request) => {
    if (
      request.method() === 'GET' &&
      request.url().includes('/api/v1/nodes/')
    ) {
      nodeRequests += 1
    }
  })
  const before = nodeRequests
  const filter = page.getByRole('textbox', { name: '筛选当前文件夹' })
  await filter.fill(folderB)
  await expect(fileRow(page, folderB)).toBeVisible()
  await expect(fileRow(page, folderRenamed)).toHaveCount(0)
  await page.waitForTimeout(500)
  expect(nodeRequests).toBe(before)
  await filter.fill('')
  await expect(fileRow(page, folderRenamed)).toBeVisible()

  // localStorage 只包含视图模式与排序两个键，且值中不含文件名。
  const stored = await page.evaluate(() => {
    const entries: Record<string, string> = {}
    for (let index = 0; index < window.localStorage.length; index += 1) {
      const key = window.localStorage.key(index)
      if (key) {
        entries[key] = window.localStorage.getItem(key) ?? ''
      }
    }
    return entries
  })
  expect(Object.keys(stored).sort()).toEqual([
    'backup.sort',
    'backup.viewMode',
  ])
  const values = Object.values(stored).join('|')
  expect(values).not.toContain(folderRenamed)
  expect(values).not.toContain(folderB)

  // 旧地址重定向到账号设置对应分区（刷新后需要先解锁）。
  await page.goto('/sessions')
  await unlockWithPassword(page)
  await expect(page).toHaveURL(/\/settings#sessions$/)
  await expect(page.getByRole('tab', { name: '登录设备' })).toHaveAttribute(
    'aria-selected',
    'true',
  )

  // `/` 进入文件页。
  await page.goto('/')
  await unlockWithPassword(page)
  await expect(page).toHaveURL(/\/files$/)
  await expect(page.getByRole('heading', { name: '全部文件' })).toBeVisible()

  // 退出登录后 Worker 内会话结束，回到登录页。
  await page.getByRole('button', { name: `账号菜单 ${email}` }).click()
  await page.getByRole('menuitem', { name: '退出登录' }).click()
  await expect(
    page.getByRole('heading', { name: '登录' }),
  ).toBeVisible()
  await expect(page).toHaveURL(/\/login$/)
})
