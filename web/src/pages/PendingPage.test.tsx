import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import { describe, expect, it, vi } from 'vitest'

import { AuthContext, type AuthContextValue } from '../auth/context'
import type { CurrentUser } from '../worker/types'
import { PendingPage } from './PendingPage'

const USER: CurrentUser = {
  userId: '550e8400-e29b-41d4-a716-446655440000',
  email: 'pending@example.com',
  emailVerified: true,
  rootNodeId: '550e8400-e29b-41d4-a716-44665544000a',
  status: 'pending',
  statusReason: null,
  isAdmin: false,
}

function authValue(overrides: Partial<AuthContextValue> = {}): AuthContextValue {
  return {
    api: null,
    user: USER,
    loading: false,
    unlocked: true,
    setSession: vi.fn(),
    markUnlocked: vi.fn(),
    logout: vi.fn(() => Promise.resolve()),
    ...overrides,
  }
}

describe('PendingPage', () => {
  it('显示等待审核说明、邮箱和退出登录入口', async () => {
    const logout = vi.fn(() => Promise.resolve())
    render(
      <MemoryRouter>
        <AuthContext.Provider value={authValue({ logout })}>
          <PendingPage />
        </AuthContext.Provider>
      </MemoryRouter>,
    )

    expect(screen.getByTestId('pending-page')).toBeInTheDocument()
    expect(
      screen.getByRole('heading', { name: '正在等待管理员审核' }),
    ).toBeInTheDocument()
    expect(screen.getByText('pending@example.com')).toBeInTheDocument()
    expect(screen.getByText(/审核通过前不能上传文件/)).toBeInTheDocument()

    await userEvent.click(screen.getByTestId('pending-logout'))
    expect(logout).toHaveBeenCalledTimes(1)
  })
})
