import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'

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

function renderPanel(items: TransferItemView[]) {
  const props = {
    items,
    onPause: vi.fn(),
    onResume: vi.fn(),
    onCancel: vi.fn(),
    onRetry: vi.fn(),
    onResumeFile: vi.fn(),
    onDismiss: vi.fn(),
    onDismissCompleted: vi.fn(),
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
    expect(screen.queryAllByTestId('transfer-item')).toHaveLength(0)
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
    expect(screen.queryByTestId('transfer-panel')).not.toBeInTheDocument()
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
})
