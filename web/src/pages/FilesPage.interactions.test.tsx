import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { AuthContext, type AuthContextValue } from '../auth/context'
import { UsageProvider } from '../shell/UsageProvider'
import { ToastProvider } from '../ui'
import type { FileNodeView, WorkerApi } from '../worker/types'
import { FilesPage } from './FilesPage'

const USER = {
  userId: '550e8400-e29b-41d4-a716-446655440000',
  email: 'user@example.com',
  emailVerified: true,
  rootNodeId: '550e8400-e29b-41d4-a716-44665544000a',
}

function node(overrides: Partial<FileNodeView>): FileNodeView {
  return {
    id: overrides.id ?? overrides.name ?? 'id',
    parentId: USER.rootNodeId,
    kind: 'file',
    name: 'file',
    size: 10,
    mtime: 100,
    mime: 'text/plain',
    rowVersion: 1,
    currentVersionId: 'version',
    createdAt: '2026-01-01T00:00:00Z',
    updatedAt: '2026-01-01T00:00:00Z',
    ...overrides,
  }
}

const ITEMS = [
  node({ id: 'b', name: 'b.txt', size: 30, mtime: 300 }),
  node({ id: 'folder', name: '文件夹', kind: 'folder', size: null, mtime: null }),
  node({ id: 'a', name: 'a.txt', size: 10, mtime: 100 }),
  node({ id: 'c', name: 'c.txt', size: 20, mtime: 200 }),
]

function renderPage(api: Partial<WorkerApi>) {
  const value: AuthContextValue = {
    api: api as WorkerApi,
    user: USER,
    loading: false,
    unlocked: true,
    setSession: vi.fn(),
    markUnlocked: vi.fn(),
    logout: vi.fn(),
  }
  return render(
    <MemoryRouter>
      <AuthContext.Provider value={value}>
        <UsageProvider>
          <ToastProvider>
            <FilesPage />
          </ToastProvider>
        </UsageProvider>
      </AuthContext.Provider>
    </MemoryRouter>,
  )
}

function api(overrides: Partial<WorkerApi> = {}): Partial<WorkerApi> {
  return {
    listFolder: vi.fn().mockResolvedValue({ items: ITEMS, nextCursor: null }),
    getUsage: vi.fn().mockResolvedValue({
      usedBytes: 0,
      reservedBytes: 0,
      quotaBytes: 1024,
    }),
    listPendingUploads: vi.fn().mockResolvedValue([]),
    ...overrides,
  }
}

function rowNames(): (string | null)[] {
  return screen
    .getAllByTestId('file-row')
    .map((row) => row.getAttribute('data-name'))
}

afterEach(() => {
  window.localStorage.clear()
})

describe('文件列表交互', () => {
  it('点击大小列头升序，再次点击降序，文件夹始终在最前', async () => {
    const user = userEvent.setup()
    renderPage(api())
    await screen.findByText('b.txt')

    await user.click(screen.getByRole('button', { name: '大小' }))
    expect(rowNames()).toEqual(['文件夹', 'a.txt', 'c.txt', 'b.txt'])

    await user.click(screen.getByRole('button', { name: '大小' }))
    expect(rowNames()).toEqual(['文件夹', 'b.txt', 'c.txt', 'a.txt'])
  })

  it('切换网格视图后写入本地偏好，重新挂载仍然保持', async () => {
    const user = userEvent.setup()
    const first = renderPage(api())
    await screen.findByText('b.txt')
    await user.click(screen.getByTestId('view-grid'))
    expect(screen.getByTestId('file-grid')).toBeVisible()
    expect(window.localStorage.getItem('backup.viewMode')).toBe('grid')
    first.unmount()

    renderPage(api())
    await screen.findByText('b.txt')
    expect(screen.getByTestId('file-grid')).toBeVisible()
  })

  it('筛选只在本地过滤，不新增网络请求', async () => {
    const listFolder = vi.fn().mockResolvedValue({
      items: ITEMS,
      nextCursor: null,
    })
    const user = userEvent.setup()
    renderPage(api({ listFolder }))
    await screen.findByText('b.txt')
    await waitFor(() => expect(listFolder).toHaveBeenCalledTimes(1))

    await user.type(
      screen.getByRole('textbox', { name: '筛选当前文件夹' }),
      'b.',
    )
    expect(rowNames()).toEqual(['b.txt'])
    expect(listFolder).toHaveBeenCalledTimes(1)

    await user.clear(
      screen.getByRole('textbox', { name: '筛选当前文件夹' }),
    )
    expect(rowNames()).toHaveLength(4)
    expect(listFolder).toHaveBeenCalledTimes(1)
  })

  it('“更多”菜单与右键菜单提供相同操作，Esc 关闭', async () => {
    const user = userEvent.setup()
    renderPage(api())
    await screen.findByText('b.txt')

    await user.click(screen.getByTestId('more-b'))
    expect(
      screen.getByRole('menuitem', { name: '重命名 b.txt' }),
    ).toBeVisible()
    expect(
      screen.getByRole('menuitem', { name: '删除 b.txt' }),
    ).toBeVisible()
    await user.keyboard('{Escape}')
    expect(screen.queryByRole('menu')).not.toBeInTheDocument()

    const row = screen
      .getAllByTestId('file-row')
      .find((element) => element.getAttribute('data-name') === 'b.txt')
    expect(row).toBeTruthy()
    fireEvent.contextMenu(row as HTMLElement, { clientX: 40, clientY: 50 })
    expect(
      screen.getByRole('menuitem', { name: '移动 b.txt' }),
    ).toBeVisible()
    await user.keyboard('{Escape}')
    expect(screen.queryByRole('menu')).not.toBeInTheDocument()
  })

  it('本地存储只包含视图模式与排序两个键', async () => {
    const user = userEvent.setup()
    renderPage(api())
    await screen.findByText('b.txt')
    await user.click(screen.getByRole('button', { name: '大小' }))
    await user.click(screen.getByTestId('view-grid'))

    expect(Object.keys(window.localStorage).sort()).toEqual([
      'backup.sort',
      'backup.viewMode',
    ])
  })
})
