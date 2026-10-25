import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import { describe, expect, it, vi } from 'vitest'

import { AuthContext, type AuthContextValue } from '../auth/context'
import { UsageProvider } from '../shell/UsageProvider'
import { ToastProvider } from '../ui'
import type { TrashNodeView, WorkerApi } from '../worker/types'
import { TrashView } from './TrashView'

const USER = {
  userId: '550e8400-e29b-41d4-a716-446655440000',
  email: 'user@example.com',
  emailVerified: true,
  rootNodeId: '550e8400-e29b-41d4-a716-44665544000a',
  status: 'active' as const,
  statusReason: null,
  isAdmin: false,
}

function trashItem(overrides: Partial<TrashNodeView> = {}): TrashNodeView {
  return {
    id: 'trash-1',
    parentId: USER.rootNodeId,
    kind: 'file',
    name: '旧合同.pdf',
    size: 2048,
    mtime: 1_700_000_000,
    mime: 'application/pdf',
    hasContentSha256: false,
    rowVersion: 1,
    currentVersionId: 'version-1',
    createdAt: '2026-01-01T00:00:00Z',
    updatedAt: '2026-01-01T00:00:00Z',
    deletedAt: '2026-02-01T12:00:00Z',
    trashBatchId: 'batch-1',
    ...overrides,
  }
}

function renderTrash(api: Partial<WorkerApi>) {
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
    <MemoryRouter initialEntries={['/trash']}>
      <AuthContext.Provider value={value}>
        <UsageProvider>
          <ToastProvider>
            <TrashView />
          </ToastProvider>
        </UsageProvider>
      </AuthContext.Provider>
    </MemoryRouter>,
  )
}

function baseApi(overrides: Partial<WorkerApi> = {}): Partial<WorkerApi> {
  return {
    listTrash: vi.fn().mockResolvedValue([trashItem()]),
    getUsage: vi.fn().mockResolvedValue({
      usedBytes: 0,
      reservedBytes: 0,
      quotaBytes: 1024,
    }),
    ...overrides,
  }
}

describe('回收站视图', () => {
  it('列出删除项的名称、删除时间与大小', async () => {
    renderTrash(baseApi())
    const row = await screen.findByTestId('trash-item')
    expect(row).toHaveAttribute('data-name', '旧合同.pdf')
    expect(row).toHaveTextContent('旧合同.pdf')
    expect(row).toHaveTextContent('2.0 KiB')
    expect(
      screen.getByRole('button', { name: '恢复 旧合同.pdf' }),
    ).toBeVisible()
    expect(
      screen.getByRole('button', { name: '永久删除 旧合同.pdf' }),
    ).toBeVisible()
  })

  it('恢复后从回收站移除并提示', async () => {
    const user = userEvent.setup()
    const restoreNode = vi.fn().mockResolvedValue(trashItem())
    const listTrash = vi
      .fn()
      .mockResolvedValueOnce([trashItem()])
      .mockResolvedValue([])
    renderTrash(baseApi({ restoreNode, listTrash }))

    await screen.findByTestId('trash-item')
    await user.click(
      screen.getByRole('button', { name: '恢复 旧合同.pdf' }),
    )
    await waitFor(() => expect(restoreNode).toHaveBeenCalledWith('trash-1'))
    await waitFor(() =>
      expect(screen.queryByTestId('trash-item')).not.toBeInTheDocument(),
    )
    expect(screen.getByRole('status')).toHaveTextContent('已恢复')
  })

  it('永久删除必须经过确认对话框', async () => {
    const user = userEvent.setup()
    const permanentlyDeleteNode = vi.fn().mockResolvedValue({
      deletedNodes: 1,
      deletedVersions: 1,
      deletedContents: 1,
      freedBytes: 2048,
      queuedObjects: 1,
    })
    const listTrash = vi
      .fn()
      .mockResolvedValueOnce([trashItem()])
      .mockResolvedValue([])
    renderTrash(baseApi({ permanentlyDeleteNode, listTrash }))

    await screen.findByTestId('trash-item')
    await user.click(
      screen.getByRole('button', { name: '永久删除 旧合同.pdf' }),
    )
    expect(
      screen.getByRole('dialog', { name: '永久删除' }),
    ).toBeVisible()
    expect(permanentlyDeleteNode).not.toHaveBeenCalled()

    await user.click(screen.getByTestId('confirm-permanent-delete'))
    await waitFor(() =>
      expect(permanentlyDeleteNode).toHaveBeenCalledWith('trash-1'),
    )
    expect(screen.getByRole('status')).toHaveTextContent('已永久删除')
  })

  it('原位置不可用时打开文件夹选择器', async () => {
    const user = userEvent.setup()
    const restoreNode = vi.fn().mockRejectedValue(
      Object.assign(new Error('target unavailable'), {
        code: 'target_unavailable',
      }),
    )
    const listFolder = vi.fn().mockResolvedValue({
      items: [trashItem({ id: 'folder-1', kind: 'folder', name: '归档' })],
      nextCursor: null,
    })
    renderTrash(baseApi({ restoreNode, listFolder }))

    await screen.findByTestId('trash-item')
    await user.click(
      screen.getByRole('button', { name: '恢复 旧合同.pdf' }),
    )
    expect(
      await screen.findByRole('dialog', {
        name: '恢复“旧合同.pdf”到新位置',
      }),
    ).toBeVisible()
    expect(await screen.findByTestId('folder-picker')).toBeVisible()
  })

  it('空回收站显示空状态', async () => {
    renderTrash(
      baseApi({ listTrash: vi.fn().mockResolvedValue([]) }),
    )
    expect(await screen.findByText('回收站是空的')).toBeVisible()
  })
})
