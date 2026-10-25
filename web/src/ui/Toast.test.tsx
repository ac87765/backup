import { act, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { ToastProvider } from './Toast'
import { useToast } from './toast-context'

function Trigger() {
  const { showToast } = useToast()
  return (
    <button type="button" onClick={() => showToast('已恢复')}>
      显示提示
    </button>
  )
}

describe('Toast', () => {
  afterEach(() => {
    vi.useRealTimers()
  })

  it('以 role="status" 展示提示并在 3 秒后自动消失', () => {
    vi.useFakeTimers()
    render(
      <ToastProvider>
        <Trigger />
      </ToastProvider>,
    )
    fireEvent.click(screen.getByRole('button', { name: '显示提示' }))
    expect(screen.getByRole('status')).toHaveTextContent('已恢复')

    act(() => {
      vi.advanceTimersByTime(3_000)
    })
    expect(screen.queryByRole('status')).not.toBeInTheDocument()
  })

  it('支持同时显示多条提示', () => {
    render(
      <ToastProvider>
        <Trigger />
      </ToastProvider>,
    )
    fireEvent.click(screen.getByRole('button', { name: '显示提示' }))
    fireEvent.click(screen.getByRole('button', { name: '显示提示' }))
    expect(screen.getAllByTestId('toast')).toHaveLength(2)
  })
})
