import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import App from './App'

describe('App', () => {
  it('渲染应用外壳', () => {
    render(<App />)

    expect(
      screen.getByRole('heading', { level: 1, name: '端对端加密备份' }),
    ).toBeInTheDocument()
  })
})
