import { useState } from 'react'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MoreHorizontal } from 'lucide-react'
import { describe, expect, it, vi } from 'vitest'

import { IconButton } from './IconButton'
import { ContextMenu, Menu, type MenuItem } from './Menu'

function items(onSelect: (value: string) => void): MenuItem[] {
  return [
    {
      label: '重命名',
      ariaLabel: '重命名 合同.pdf',
      onSelect: () => onSelect('rename'),
    },
    {
      label: '移动到…',
      ariaLabel: '移动 合同.pdf',
      onSelect: () => onSelect('move'),
    },
    {
      label: '删除',
      ariaLabel: '删除 合同.pdf',
      tone: 'danger',
      onSelect: () => onSelect('delete'),
    },
  ]
}

function MenuHarness() {
  const [value, setValue] = useState('')
  return (
    <>
      <Menu
        label="更多操作 合同.pdf"
        trigger={
          <IconButton label="更多 合同.pdf">
            <MoreHorizontal />
          </IconButton>
        }
        items={items(setValue)}
      />
      <span data-testid="value">{value}</span>
    </>
  )
}

describe('Menu', () => {
  it('Enter 打开后用方向键移动焦点并选择菜单项', async () => {
    const user = userEvent.setup()
    render(<MenuHarness />)
    const trigger = screen.getByRole('button', { name: '更多 合同.pdf' })
    trigger.focus()

    await user.keyboard('{Enter}')
    expect(screen.getByRole('menu')).toBeVisible()
    expect(
      screen.getByRole('menuitem', { name: '重命名 合同.pdf' }),
    ).toHaveFocus()

    await user.keyboard('{ArrowDown}{ArrowUp}{Enter}')
    expect(screen.getByTestId('value')).toHaveTextContent('rename')
    await waitFor(() =>
      expect(screen.queryByRole('menu')).not.toBeInTheDocument(),
    )
    expect(trigger).toHaveFocus()
  })

  it('Esc 关闭菜单并把焦点还给触发按钮', async () => {
    const user = userEvent.setup()
    render(<MenuHarness />)
    const trigger = screen.getByRole('button', { name: '更多 合同.pdf' })

    await user.click(trigger)
    expect(screen.getByRole('menu')).toBeVisible()
    await user.keyboard('{Escape}')
    await waitFor(() =>
      expect(screen.queryByRole('menu')).not.toBeInTheDocument(),
    )
    expect(trigger).toHaveFocus()
  })

  it('点击菜单外部关闭', async () => {
    const user = userEvent.setup()
    render(
      <>
        <MenuHarness />
        <button type="button">外部</button>
      </>,
    )
    await user.click(screen.getByRole('button', { name: '更多 合同.pdf' }))
    expect(screen.getByRole('menu')).toBeVisible()
    await user.click(screen.getByRole('button', { name: '外部' }))
    await waitFor(() =>
      expect(screen.queryByRole('menu')).not.toBeInTheDocument(),
    )
  })
})

describe('ContextMenu', () => {
  it('在指针位置渲染相同菜单项，Esc 关闭', async () => {
    const user = userEvent.setup()
    const onClose = vi.fn()
    render(
      <ContextMenu
        label="更多操作 合同.pdf"
        items={items(() => undefined)}
        point={{ x: 40, y: 60 }}
        onClose={onClose}
      />,
    )
    expect(screen.getByRole('menu')).toBeVisible()
    expect(
      screen.getByRole('menuitem', { name: '删除 合同.pdf' }),
    ).toBeInTheDocument()
    await user.keyboard('{Escape}')
    expect(onClose).toHaveBeenCalledTimes(1)
  })
})
