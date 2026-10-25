import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { describe, expect, it, vi } from 'vitest'

import { AuthContext, type AuthContextValue } from '../auth/context'
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
    id: '550e8400-e29b-41d4-a716-446655440001',
    parentId: USER.rootNodeId,
    kind: 'folder',
    name: '文件夹',
    size: null,
    mtime: null,
    mime: null,
    rowVersion: 1,
    currentVersionId: null,
    createdAt: '2026-01-01T00:00:00Z',
    updatedAt: '2026-01-01T00:00:00Z',
    ...overrides,
  }
}

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
        <FilesPage />
      </AuthContext.Provider>
    </MemoryRouter>,
  )
}

describe('文件浏览器页面', () => {
  it('显示 SQL/Worker 返回的文件节点并按解密名称本地排序', async () => {
    const listFolder = vi.fn().mockResolvedValue({
      items: [
        node({ id: '550e8400-e29b-41d4-a716-446655440002', name: 'Z 文件夹' }),
        node({
          id: '550e8400-e29b-41d4-a716-446655440003',
          kind: 'file',
          name: 'A 文件.txt',
          size: 1024,
          mtime: 1_700_000_000,
          mime: 'text/plain',
          currentVersionId: '550e8400-e29b-41d4-a716-446655440004',
        }),
      ],
      nextCursor: null,
    })
    const getUsage = vi.fn().mockResolvedValue({
      usedBytes: 1024,
      reservedBytes: 0,
      quotaBytes: 1024 * 1024,
    })
    renderPage({ listFolder, getUsage })

    await screen.findByText('A 文件.txt')
    const rows = screen.getAllByTestId('file-row')
    expect(rows.map((row) => row.getAttribute('data-name'))).toEqual([
      'A 文件.txt',
      'Z 文件夹',
    ])
    expect(screen.getByText(/1\.0 KiB ·/)).toBeInTheDocument()
    expect(screen.getByTestId('usage-summary')).toHaveTextContent(
      '已用 1.0 KiB',
    )
  })

  it('空名称在客户端被拒绝，不发创建请求', async () => {
    const listFolder = vi.fn().mockResolvedValue({
      items: [],
      nextCursor: null,
    })
    const createFolder = vi.fn()
    const getUsage = vi.fn().mockResolvedValue({
      usedBytes: 0,
      reservedBytes: 0,
      quotaBytes: 1024,
    })
    renderPage({ listFolder, createFolder, getUsage })

    await waitFor(() => expect(listFolder).toHaveBeenCalled())
    fireEvent.click(screen.getByTestId('new-folder'))
    fireEvent.click(screen.getByTestId('create-folder-submit'))
    expect(await screen.findByRole('alert')).toHaveTextContent('名称不能为空')
    expect(createFolder).not.toHaveBeenCalled()
  })
})
