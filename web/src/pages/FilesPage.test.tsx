import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import { describe, expect, it, vi } from 'vitest'

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
        <UsageProvider>
          <ToastProvider>
            <FilesPage />
          </ToastProvider>
        </UsageProvider>
      </AuthContext.Provider>
    </MemoryRouter>,
  )
}

describe('文件浏览器页面', () => {
  it('文件夹排在文件之前，并按解密名称显示与排序', async () => {
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
      'Z 文件夹',
      'A 文件.txt',
    ])
    expect(screen.getAllByText(/1\.0 KiB/).length).toBeGreaterThan(0)
    expect(await screen.findByTestId('usage-summary')).toHaveTextContent(
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
    fireEvent.click(screen.getByTestId('upload-menu'))
    fireEvent.click(await screen.findByTestId('new-folder'))
    fireEvent.click(screen.getByTestId('create-folder-submit'))
    expect(await screen.findByRole('alert')).toHaveTextContent('名称不能为空')
    expect(createFolder).not.toHaveBeenCalled()
  })

  it('选择文件后进入传输队列并显示完成进度', async () => {
    const listFolder = vi.fn().mockResolvedValue({
      items: [],
      nextCursor: null,
    })
    const getUsage = vi.fn().mockResolvedValue({
      usedBytes: 0,
      reservedBytes: 0,
      quotaBytes: 1024 * 1024,
    })
    const uploadFile = vi.fn(
      (
        parentId: string,
        file: File,
        onProgress?: (progress: {
          transferId: string
          direction: 'upload'
          fileName: string
          status: 'uploading' | 'completed'
          completedChunks: number
          totalChunks: number
          completedBytes: number
          totalBytes: number
        }) => void,
      ) => {
        expect(parentId).toBe(USER.rootNodeId)
        onProgress?.({
          transferId: 'session-1',
          direction: 'upload',
          fileName: file.name,
          status: 'uploading',
          completedChunks: 0,
          totalChunks: 1,
          completedBytes: 0,
          totalBytes: file.size,
        })
        onProgress?.({
          transferId: 'session-1',
          direction: 'upload',
          fileName: file.name,
          status: 'completed',
          completedChunks: 1,
          totalChunks: 1,
          completedBytes: file.size,
          totalBytes: file.size,
        })
        return Promise.resolve({
          nodeId: 'node-1',
          versionId: 'version-1',
          contentId: 'content-1',
          name: file.name,
          size: file.size,
        })
      },
    )
    renderPage({
      listFolder,
      getUsage,
      uploadFile,
      listPendingUploads: vi.fn().mockResolvedValue([]),
    })

    const file = new File([new Uint8Array(4)], '上传.txt', {
      type: 'text/plain',
    })
    await userEvent.upload(screen.getByTestId('upload-input'), file)
    await waitFor(() => {
      expect(screen.getByTestId('transfer-item')).toHaveAttribute(
        'data-status',
        'completed',
      )
    })
    expect(uploadFile).toHaveBeenCalledTimes(1)
  })
})
