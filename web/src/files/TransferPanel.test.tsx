import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'

import type { FolderJobView } from './folder-job'
import { TransferPanel, type TransferItemView } from './TransferPanel'

function item(overrides: Partial<TransferItemView>): TransferItemView {
  return {
    localId: 'local-1',
    direction: 'upload',
    fileName: '报告.pdf',
    status: 'uploading',
    completedChunks: 1,
    totalChunks: 4,
    completedBytes: 25,
    totalBytes: 100,
    ...overrides,
  }
}

function folderJob(overrides: Partial<FolderJobView> = {}): FolderJobView {
  return {
    id: 'folder-job-1',
    name: '照片',
    kind: 'folder',
    direction: 'upload',
    status: 'running',
    doneFiles: 2,
    totalFiles: 5,
    completedBytes: 20,
    totalBytes: 100,
    counts: { added: 1, updated: 1, unchanged: 0, failed: 1 },
    children: [
      {
        id: 'a',
        name: 'a.jpg',
        relativePath: '2025/a.jpg',
        status: 'uploading',
      },
      {
        id: 'b',
        name: 'b.jpg',
        relativePath: 'b.jpg',
        status: 'failed',
        error: '网络中断',
      },
    ],
    hiddenChildren: 0,
    retrying: false,
    retryable: true,
    ...overrides,
  }
}

function renderPanel(
  items: TransferItemView[],
  folderJobs: FolderJobView[] = [],
) {
  const props = {
    items,
    folderJobs,
    onPause: vi.fn(),
    onResume: vi.fn(),
    onCancel: vi.fn(),
    onRetry: vi.fn(),
    onResumeFile: vi.fn(),
    onDismiss: vi.fn(),
    onDismissCompleted: vi.fn(),
    onCancelFolder: vi.fn(),
    onRetryFolder: vi.fn(),
    onDismissFolder: vi.fn(),
  }
  const view = render(<TransferPanel {...props} />)
  return { ...props, ...view }
}

describe('TransferPanel', () => {
  it('显示进行中数量与总体进度，折叠后保留标题进度', async () => {
    const user = userEvent.setup()
    const first = item({ localId: 'a', fileName: 'a.bin', completedBytes: 0 })
    const second = item({
      localId: 'b',
      fileName: 'b.bin',
      completedBytes: 100,
    })
    const { rerender, ...props } = renderPanel([first, second])

    expect(
      screen.getByText('正在传输 2 项 · 50%'),
    ).toBeVisible()
    expect(screen.getAllByTestId('transfer-item')).toHaveLength(2)

    await user.click(screen.getByRole('button', { name: '折叠传输面板' }))
    expect(screen.getByTestId('transfer-panel')).toHaveAttribute(
      'data-collapsed',
      'true',
    )
    expect(screen.getByText('正在传输 2 项 · 50%')).toBeVisible()

    rerender(
      <TransferPanel
        {...props}
        items={[
          { ...first, status: 'completed', completedBytes: 100 },
          { ...second, status: 'completed', completedBytes: 100 },
        ]}
      />,
    )
    expect(screen.getByText('传输完成')).toBeVisible()
  })

  it('失败项显示原因与重试按钮', async () => {
    const user = userEvent.setup()
    const failed = item({
      status: 'failed',
      errorMessage: '网络中断',
    })
    const { onRetry } = renderPanel([failed])

    expect(screen.getByRole('alert')).toHaveTextContent('网络中断')
    await user.click(screen.getByTestId('transfer-retry'))
    expect(onRetry).toHaveBeenCalledWith(failed)
  })

  it('全部完成后提供“清除已完成”', async () => {
    const user = userEvent.setup()
    const done = item({ status: 'completed', completedBytes: 100 })
    const { onDismissCompleted } = renderPanel([done])

    expect(screen.getByText('传输完成')).toBeVisible()
    await user.click(screen.getByTestId('transfer-clear-completed'))
    expect(onDismissCompleted).toHaveBeenCalledTimes(1)
  })

  it('关闭后收起为可重新打开的浮动按钮', async () => {
    const user = userEvent.setup()
    renderPanel([item({})])

    await user.click(screen.getByRole('button', { name: '关闭传输面板' }))
    await waitFor(() =>
      expect(screen.queryByTestId('transfer-panel')).not.toBeInTheDocument(),
    )
    await user.click(screen.getByRole('button', { name: '打开传输面板' }))
    expect(screen.getByTestId('transfer-panel')).toBeVisible()
  })

  it('保留 transfer-* 钩子与状态属性', () => {
    renderPanel([
      item({
        localId: 'a',
        status: 'paused',
        sessionId: 'session-1',
        pendingResume: true,
      }),
    ])
    const row = screen.getByTestId('transfer-item')
    expect(row).toHaveAttribute('data-status', 'paused')
    expect(row).toHaveAttribute('data-direction', 'upload')
    expect(row).toHaveAttribute('data-name', '报告.pdf')
    expect(screen.getByTestId('transfer-progress')).toBeInTheDocument()
    expect(
      screen.getByTestId('resume-upload-input-session-1'),
    ).toBeInTheDocument()
  })

  it('文件夹任务分组显示计数、摘要、勾子与展开的失败文件', async () => {
    const user = userEvent.setup()
    const job = folderJob()
    const { onCancelFolder } = renderPanel([], [job])

    const row = screen.getByTestId('folder-job')
    expect(row).toHaveAttribute('data-status', 'running')
    expect(row).toHaveAttribute('data-done', '2')
    expect(row).toHaveAttribute('data-total', '5')
    expect(row).toHaveAttribute('data-direction', 'upload')
    expect(row).toHaveTextContent('已完成 2 / 5 个文件')
    expect(row).toHaveTextContent('新增 1 · 更新 1 · 未变化 0 · 失败 1')

    await user.click(screen.getAllByTestId('folder-job-expand')[0])
    expect(screen.getAllByTestId('folder-job-child')).toHaveLength(2)
    expect(screen.getByText('网络中断')).toBeVisible()

    await user.click(screen.getByTestId('folder-job-cancel'))
    expect(onCancelFolder).toHaveBeenCalledWith(job)

    const failed = folderJob({
      status: 'failed',
      retryable: true,
      doneFiles: 5,
    })
    const retryRender = renderPanel([], [failed])
    await user.click(screen.getByTestId('folder-job-retry'))
    expect(retryRender.onRetryFolder).toHaveBeenCalledWith(failed)
  })
})
