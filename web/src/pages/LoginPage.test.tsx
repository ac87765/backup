import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { describe, expect, it, vi } from 'vitest'

import { AuthContext, type AuthContextValue } from '../auth/context'
import type { WorkerApi } from '../worker/types'
import { LoginPage } from './LoginPage'

function renderLogin(login: WorkerApi['login']) {
  const value: AuthContextValue = {
    api: { login } as unknown as WorkerApi,
    user: null,
    loading: false,
    unlocked: false,
    setSession: vi.fn(),
    markUnlocked: vi.fn(),
    logout: vi.fn(),
  }
  return render(
    <MemoryRouter initialEntries={['/login']}>
      <AuthContext.Provider value={value}>
        <Routes>
          <Route path="/login" element={<LoginPage />} />
          <Route path="/files" element={<p>文件页</p>} />
        </Routes>
      </AuthContext.Provider>
    </MemoryRouter>,
  )
}

describe('登录页', () => {
  it('密码错误时在表单顶部显示错误，密码框保持可编辑且按钮恢复可用', async () => {
    const user = userEvent.setup()
    const login = vi.fn().mockRejectedValue(new Error('邮箱或密码错误'))
    renderLogin(login)

    expect(
      screen.getByRole('heading', { name: '端对端加密备份' }),
    ).toBeVisible()
    await user.type(screen.getByTestId('login-email'), 'user@example.com')
    await user.type(screen.getByTestId('login-password'), 'wrong password')
    await user.click(screen.getByTestId('login-submit'))

    expect(await screen.findByRole('alert')).toHaveTextContent(
      '邮箱或密码错误',
    )
    expect(screen.getByTestId('login-password')).toHaveValue('wrong password')
    expect(screen.getByTestId('login-submit')).toBeEnabled()
  })
})
