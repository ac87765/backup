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
    hasContentSha256: false,
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
    await waitFor(() =>
      expect(screen.queryByRole('menu')).not.toBeInTheDocument(),
    )

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

  it('通过更多菜单删除文件并提示已移入回收站', async () => {
    const user = userEvent.setup()
    const deleteNode = vi.fn().mockResolvedValue(undefined)
    const listFolder = vi
      .fn()
      .mockResolvedValueOnce({ items: ITEMS, nextCursor: null })
      .mockResolvedValue({
        items: ITEMS.filter((entry) => entry.id !== 'b'),
        nextCursor: null,
      })
    renderPage(api({ deleteNode, listFolder }))
    await screen.findByText('b.txt')
    const row = screen
      .getAllByTestId('file-row')
      .find((element) => element.getAttribute('data-name') === 'b.txt')

    await user.click(screen.getByTestId('more-b'))
    await user.click(screen.getByRole('menuitem', { name: '删除 b.txt' }))
    await waitFor(() => expect(deleteNode).toHaveBeenCalledWith('b'))
    expect(await screen.findByRole('status')).toHaveTextContent(
      '已移入回收站',
    )
    await waitFor(() => expect(row).toHaveAttribute('data-removing', 'true'))
    // 淡出收起动画结束后，列表与服务端结果一致。
    await waitFor(
      () => {
        const names = screen
          .getAllByTestId('file-row')
          .map((element) => element.getAttribute('data-name'))
        expect(names).not.toContain('b.txt')
        expect(names).toEqual(['文件夹', 'a.txt', 'c.txt'])
      },
      { timeout: 2_000 },
    )
  })

  it('拖入文件时显示覆盖层，松开后上传到当前文件夹', async () => {
    const uploadFile = vi.fn().mockResolvedValue({
      nodeId: 'node-1',
      versionId: 'version-1',
      contentId: 'content-1',
      name: '拖拽.txt',
      size: 5,
    })
    renderPage(api({ uploadFile }))
    await screen.findByText('b.txt')

    const file = new File(['hello'], '拖拽.txt', { type: 'text/plain' })
    fireEvent.dragEnter(window, {
      dataTransfer: { types: ['Files'], files: [file] },
    })
    expect(screen.getByTestId('drop-overlay')).toHaveTextContent(
      '松开以上传到“全部文件”',
    )

    fireEvent.drop(window, {
      dataTransfer: { types: ['Files'], files: [file] },
    })
    expect(screen.queryByTestId('drop-overlay')).not.toBeInTheDocument()
    await waitFor(() => {
      expect(uploadFile).toHaveBeenCalledWith(
        USER.rootNodeId,
        file,
        expect.any(Function),
      )
    })
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
