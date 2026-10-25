import { render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { HeroEncryptionDiagram } from './HeroEncryptionDiagram'

describe('首屏原理图', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('暴露可访问的文字描述，默认挂载循环动画 class 且不使用 SMIL', () => {
    const { container } = render(<HeroEncryptionDiagram />)

    const svg = screen.getByRole('img', { name: /原理图/ })
    expect(svg).toBeVisible()
    expect(svg).toHaveAttribute('data-motion', 'animated')
    expect(svg).toHaveClass('security-hero-animated')
    expect(svg).toHaveAttribute('stroke-width', '1.75')
    expect(container.querySelector('animate')).toBeNull()
    expect(container.querySelector('animateTransform')).toBeNull()
  })

  it('减少动态效果时直接显示静帧，不挂动画 class', () => {
    vi.stubGlobal(
      'matchMedia',
      vi.fn().mockReturnValue({
        matches: true,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      }),
    )

    const { container } = render(<HeroEncryptionDiagram />)

    const svg = screen.getByRole('img', { name: /原理图/ })
    expect(svg).toHaveAttribute('data-motion', 'static')
    expect(svg).not.toHaveClass('security-hero-animated')
    expect(container.querySelector('animate')).toBeNull()
  })
})
