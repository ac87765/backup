import type { ReactNode } from 'react'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { describe, expect, it, vi } from 'vitest'

import { AuthContext, type AuthContextValue } from '../auth/context'
import { ToastProvider } from '../ui'
import type { UsageView, WorkerApi } from '../worker/types'
import { AppShell } from './AppShell'
import { UsageProvider } from './UsageProvider'

const USER = {
  userId: '550e8400-e29b-41d4-a716-446655440000',
  email: 'user@example.com',
  emailVerified: true,
  rootNodeId: '550e8400-e29b-41d4-a716-44665544000a',
}

function authValue(overrides: Partial<AuthContextValue> = {}): AuthContextValue {
  return {
    api: { getUsage: vi.fn().mockResolvedValue(usage(1024)) } as unknown as WorkerApi,
    user: USER,
    loading: false,
    unlocked: true,
    setSession: vi.fn(),
    markUnlocked: vi.fn(),
    logout: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  }
}

function usage(usedBytes: number): UsageView {
  return { usedBytes, reservedBytes: 0, quotaBytes: 100 }
}

function Shell({ children }: { children: ReactNode }) {
  return (
    <AppShell breadcrumb={<span>我的文件</span>} onUploadRequest={vi.fn()}>
      {children}
    </AppShell>
  )
}

function renderShell(value: AuthContextValue) {
  return render(
    <MemoryRouter initialEntries={['/files']}>
      <AuthContext.Provider value={value}>
        <UsageProvider>
          <ToastProvider>
            <Routes>
            <Route
              path="/files"
              element={
                <Shell>
                  <p>文件内容</p>
                </Shell>
              }
            />
            <Route
              path="/trash"
              element={
                <Shell>
                  <p>回收站内容</p>
                </Shell>
              }
            />
            <Route
              path="/settings"
              element={
                <Shell>
                  <p>账号设置内容</p>
                </Shell>
              }
            />
            <Route path="/login" element={<p>登录页</p>} />
            </Routes>
          </ToastProvider>
        </UsageProvider>
      </AuthContext.Provider>
    </MemoryRouter>,
  )
}

describe('AppShell', () => {
  it('展示品牌、导航、用量，并使“全部文件”处于选中态', async () => {
    const value = authValue()
    renderShell(value)

    expect(await screen.findByText('加密备份')).toBeVisible()
    expect(screen.getByTestId('home-email')).toHaveTextContent(USER.email)
    expect(screen.getByRole('link', { name: '全部文件' })).toHaveAttribute(
      'aria-current',
      'page',
    )
    await waitFor(() => {
      expect(screen.getByTestId('usage-summary')).toHaveTextContent(
        '已用 1.0 KiB / 共 100 B',
      )
    })
    expect(screen.getByRole('progressbar', { name: '存储用量' })).toBeVisible()
  })

  it('点击侧边栏“回收站”切换视图并更新选中态', async () => {
    const user = userEvent.setup()
    renderShell(authValue())

    await user.click(screen.getByTestId('open-trash'))
    expect(await screen.findByText('回收站内容')).toBeVisible()
    expect(screen.getByRole('link', { name: '回收站' })).toHaveAttribute(
      'aria-current',
      'page',
    )
    expect(screen.getByRole('link', { name: '全部文件' })).not.toHaveAttribute(
      'aria-current',
    )
  })

  it('用量达到 90% 时以警示色显示', async () => {
    const value = authValue({
      api: {
        getUsage: vi.fn().mockResolvedValue(usage(92)),
      } as unknown as WorkerApi,
    })
    renderShell(value)

    await waitFor(() => {
      expect(screen.getByTestId('usage-summary')).toHaveTextContent(
        '已用 92 B / 共 100 B',
      )
    })
    expect(screen.getByTestId('usage-summary')).toHaveClass('text-danger-600')
  })

  it('账号菜单展示邮箱、验证状态并可进入账号设置', async () => {
    const user = userEvent.setup()
    renderShell(authValue())

    await user.click(
      screen.getByRole('button', { name: `账号菜单 ${USER.email}` }),
    )
    expect(screen.getByRole('menu', { name: '账号菜单' })).toBeVisible()
    expect(screen.getByTestId('home-verified')).toHaveTextContent('邮箱已验证')

    await user.click(screen.getByRole('menuitem', { name: '账号设置' }))
    expect(await screen.findByText('账号设置内容')).toBeVisible()
  })

  it('账号菜单中的退出登录会结束会话并回到登录页', async () => {
    const user = userEvent.setup()
    const logout = vi.fn().mockResolvedValue(undefined)
    renderShell(authValue({ logout }))

    await user.click(
      screen.getByRole('button', { name: `账号菜单 ${USER.email}` }),
    )
    await user.click(screen.getByRole('menuitem', { name: '退出登录' }))
    await waitFor(() => expect(logout).toHaveBeenCalledTimes(1))
    expect(await screen.findByText('登录页')).toBeVisible()
  })

  it('移动端菜单按钮打开并关闭侧边栏抽屉', async () => {
    const user = userEvent.setup()
    renderShell(authValue())

    await user.click(screen.getByRole('button', { name: '打开导航菜单' }))
    expect(screen.getByRole('dialog', { name: '导航菜单' })).toBeVisible()
    await user.click(screen.getByRole('button', { name: '关闭导航菜单' }))
    expect(
      screen.queryByRole('dialog', { name: '导航菜单' }),
    ).not.toBeInTheDocument()
  })
})
