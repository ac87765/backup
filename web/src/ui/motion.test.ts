import { readFileSync } from 'node:fs'
import path from 'node:path'

import { afterEach, describe, expect, it, vi } from 'vitest'

import { prefersReducedMotion } from './motion'

describe('动效降级', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('prefersReducedMotion 读取系统媒体查询', () => {
    vi.stubGlobal(
      'matchMedia',
      vi.fn().mockReturnValue({
        matches: true,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      }),
    )
    expect(prefersReducedMotion()).toBe(true)
  })

  it('index.css 在 prefers-reduced-motion 下关闭动画', () => {
    const css = readFileSync(
      path.resolve(process.cwd(), 'src', 'index.css'),
      'utf8',
    )
    expect(css).toContain('prefers-reduced-motion: reduce')
    expect(css).toMatch(/animation-duration:\s*0\.01ms/)
    expect(css).toMatch(/transition-duration:\s*0\.01ms/)
    expect(css).toContain('.skeleton::after')
  })
})
