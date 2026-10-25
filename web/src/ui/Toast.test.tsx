import { act, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { ToastProvider } from './Toast'
import { useToast } from './toast-context'

function Trigger({ message = '已恢复' }: { message?: string }) {
  const { showToast } = useToast()
  return (
    <button type="button" onClick={() => showToast(message)}>
      {message}
    </button>
  )
}

function renderProvider() {
  return render(
    <ToastProvider>
      <Trigger message="已恢复" />
      <Trigger message="已重命名" />
      <Trigger message="已删除" />
      <Trigger message="已移动" />
    </ToastProvider>,
  )
}

describe('Toast', () => {
  afterEach(() => {
    vi.useRealTimers()
  })

  it('以 role="status" 展示提示，3 秒后淡出并移除', () => {
    vi.useFakeTimers()
    renderProvider()
    fireEvent.click(screen.getByRole('button', { name: '已恢复' }))
    expect(screen.getByRole('status')).toHaveTextContent('已恢复')

    act(() => {
      vi.advanceTimersByTime(3_200)
    })
    expect(screen.queryByRole('status')).not.toBeInTheDocument()
  })

  it('相同文案合并，最多同时显示 3 条', () => {
    renderProvider()
    fireEvent.click(screen.getByRole('button', { name: '已恢复' }))
    fireEvent.click(screen.getByRole('button', { name: '已恢复' }))
    expect(screen.getAllByTestId('toast')).toHaveLength(1)

    fireEvent.click(screen.getByRole('button', { name: '已重命名' }))
    fireEvent.click(screen.getByRole('button', { name: '已删除' }))
    fireEvent.click(screen.getByRole('button', { name: '已移动' }))
    const toasts = screen.getAllByTestId('toast')
    expect(toasts).toHaveLength(3)
    expect(toasts.map((toast) => toast.textContent)).not.toContain('已恢复')
  })

  it('悬停暂停计时，移出后继续', () => {
    vi.useFakeTimers()
    renderProvider()
    fireEvent.click(screen.getByRole('button', { name: '已恢复' }))
    const toast = screen.getByTestId('toast')

    fireEvent.pointerOver(toast)
    act(() => {
      vi.advanceTimersByTime(5_000)
    })
    expect(screen.getByTestId('toast')).toBeVisible()

    fireEvent.pointerOut(toast)
    act(() => {
      vi.advanceTimersByTime(3_200)
    })
    expect(screen.queryByTestId('toast')).not.toBeInTheDocument()
  })
})
