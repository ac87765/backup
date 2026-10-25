import { useState } from 'react'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it } from 'vitest'

import { Dialog } from './Dialog'

function Harness() {
  const [open, setOpen] = useState(false)
  return (
    <>
      <button type="button" onClick={() => setOpen(true)}>
        新建文件夹
      </button>
      <Dialog
        open={open}
        onClose={() => setOpen(false)}
        title="新建文件夹"
        footer={
          <button type="button" onClick={() => setOpen(false)}>
            取消
          </button>
        }
      >
        <input data-testid="folder-name" data-dialog-autofocus />
      </Dialog>
    </>
  )
}

describe('Dialog', () => {
  it('打开后焦点进入对话框，Esc 关闭并把焦点还给触发元素', async () => {
    const user = userEvent.setup()
    render(<Harness />)
    const trigger = screen.getByRole('button', { name: '新建文件夹' })
    await user.click(trigger)

    expect(screen.getByRole('dialog', { name: '新建文件夹' })).toBeVisible()
    expect(screen.getByTestId('folder-name')).toHaveFocus()

    await user.keyboard('{Escape}')
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    )
    expect(trigger).toHaveFocus()
  })

  it('点击遮罩关闭，点击内容不关闭', async () => {
    const user = userEvent.setup()
    render(<Harness />)
    await user.click(screen.getByRole('button', { name: '新建文件夹' }))
    const dialog = screen.getByRole('dialog', { name: '新建文件夹' })

    await user.click(screen.getByTestId('folder-name'))
    expect(screen.getByRole('dialog')).toBeVisible()

    await user.click(dialog)
    await waitFor(() =>
      expect(screen.queryByRole('dialog')).not.toBeInTheDocument(),
    )
  })
})
