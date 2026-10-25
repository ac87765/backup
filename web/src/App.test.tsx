import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import App from './App'

describe('App', () => {
  it('渲染应用外壳与登录页', async () => {
    render(<App />)

    expect(
      await screen.findByRole('heading', {
        level: 1,
        name: '登录',
      }),
    ).toBeInTheDocument()
    expect(screen.getByTestId('login-email')).toBeInTheDocument()
  })
})
