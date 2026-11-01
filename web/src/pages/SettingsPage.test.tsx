import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { describe, expect, it, vi } from 'vitest'

import { AuthContext, type AuthContextValue } from '../auth/context'
import { UsageProvider } from '../shell/UsageProvider'
import { ToastProvider } from '../ui'
import type { WorkerApi } from '../worker/types'
import { SettingsPage } from './SettingsPage'

const USER = {
  userId: '550e8400-e29b-41d4-a716-446655440000',
  email: 'user@example.com',
  emailVerified: true,
  rootNodeId: '550e8400-e29b-41d4-a716-44665544000a',
  status: 'active' as const,
  statusReason: null,
  isAdmin: false,
}

function renderSettings(initialEntry: string) {
  const api = {
    getUsage: vi.fn().mockResolvedValue({
      usedBytes: 0,
      reservedBytes: 0,
      quotaBytes: 1024,
    }),
    getSessions: vi.fn().mockResolvedValue([]),
  } as unknown as WorkerApi
  const value: AuthContextValue = {
    api,
    user: USER,
    loading: false,
    unlocked: true,
    setSession: vi.fn(),
    markUnlocked: vi.fn(),
    logout: vi.fn(),
  }
  return render(
    <MemoryRouter initialEntries={[initialEntry]}>
      <AuthContext.Provider value={value}>
        <UsageProvider>
          <ToastProvider>
            <Routes>
              <Route path="/settings" element={<SettingsPage />} />
            </Routes>
          </ToastProvider>
        </UsageProvider>
      </AuthContext.Provider>
    </MemoryRouter>,
  )
}

describe('账号设置页', () => {
  it('按 hash 打开对应分区并可切换三个分区', async () => {
    const user = userEvent.setup()
    renderSettings('/settings#recovery')

    expect(screen.getByTestId('rotate-current-password')).toBeVisible()
    expect(
      screen.getByRole('tab', { name: '恢复密钥' }),
    ).toHaveAttribute('aria-selected', 'true')

    await user.click(screen.getByRole('tab', { name: '修改密码' }))
    expect(await screen.findByTestId('change-old-password')).toBeVisible()

    await user.click(screen.getByRole('tab', { name: '登录设备' }))
    expect(await screen.findByText('没有活跃会话')).toBeVisible()
  })

  it('提供桌面客户端下载入口', () => {
    renderSettings('/settings')

    expect(screen.getByTestId('settings-download-client')).toHaveAttribute(
      'href',
      '/download',
    )
  })
})
