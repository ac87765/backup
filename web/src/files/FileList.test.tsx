import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'

import type { FileNodeView } from '../worker/types'
import type { FileActions } from './file-actions'
import { FileList } from './FileList'

function node(): FileNodeView {
  return {
    id: 'file-1',
    parentId: 'root',
    kind: 'file',
    name: '合同.pdf',
    size: 1024,
    mtime: 1_700_000_000,
    mime: 'application/pdf',
    hasContentSha256: false,
    rowVersion: 1,
    currentVersionId: 'version-1',
    createdAt: '2026-01-01T00:00:00Z',
    updatedAt: '2026-01-01T00:00:00Z',
  }
}

function actions(): FileActions {
  return {
    busy: false,
    onOpen: vi.fn(),
    onDownload: vi.fn(),
    onRename: vi.fn(),
    onMove: vi.fn(),
    onVersions: vi.fn(),
    onNewVersion: vi.fn(),
    onDelete: vi.fn(),
  }
}

function renderList(fileActions: FileActions) {
  return render(
    <FileList
      items={[node()]}
      sort={{ field: 'name', direction: 'asc' }}
      onSort={vi.fn()}
      selectedId={null}
      onSelect={vi.fn()}
      actions={fileActions}
      onEndReached={vi.fn()}
    />,
  )
}

describe('FileList 键盘可达性', () => {
  it('名称按钮可用 Enter 打开文件（下载）', async () => {
    const user = userEvent.setup()
    const fileActions = actions()
    renderList(fileActions)

    const name = screen.getByRole('button', { name: '打开 合同.pdf' })
    name.focus()
    await user.keyboard('{Enter}')
    expect(fileActions.onOpen).toHaveBeenCalledTimes(1)
    expect(
      screen.getByRole('button', { name: '下载 合同.pdf' }),
    ).toBeInTheDocument()
  })

  it('“更多”按钮可用 Enter 打开，方向键选择重命名', async () => {
    const user = userEvent.setup()
    const fileActions = actions()
    renderList(fileActions)

    const more = screen.getByRole('button', { name: '更多 合同.pdf' })
    more.focus()
    await user.keyboard('{Enter}')
    expect(screen.getByRole('menu')).toBeVisible()
    await user.keyboard('{ArrowDown}{ArrowUp}{Enter}')
    expect(fileActions.onRename).toHaveBeenCalledTimes(1)
    await waitFor(() =>
      expect(screen.queryByRole('menu')).not.toBeInTheDocument(),
    )
  })
})
