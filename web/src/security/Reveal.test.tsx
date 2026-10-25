import { act, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { Reveal } from './Reveal'

class MockIntersectionObserver {
  static instances: MockIntersectionObserver[] = []

  readonly root = null
  readonly rootMargin = '0px'
  readonly thresholds: number[] = []
  readonly observe = vi.fn()
  readonly unobserve = vi.fn()
  readonly disconnect = vi.fn()
  readonly takeRecords = () => []
  private readonly callback: IntersectionObserverCallback

  constructor(
    callback: IntersectionObserverCallback,
    _options?: IntersectionObserverInit,
  ) {
    this.callback = callback
    MockIntersectionObserver.instances.push(this)
  }

  emit(isIntersecting: boolean) {
    this.callback(
      [{ isIntersecting } as IntersectionObserverEntry],
      this as unknown as IntersectionObserver,
    )
  }
}

function stubObserver() {
  MockIntersectionObserver.instances = []
  vi.stubGlobal('IntersectionObserver', MockIntersectionObserver)
}

describe('区块进入视口淡入', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('不支持 IntersectionObserver 时直接显示内容', () => {
    render(<Reveal>区块内容</Reveal>)

    const wrapper = screen.getByText('区块内容')
    expect(wrapper).toHaveClass('opacity-100')
    expect(wrapper).not.toHaveClass('opacity-0')
  })

  it('进入视口后只切换 class，不留下隐藏内容', () => {
    stubObserver()
    render(<Reveal>区块内容</Reveal>)

    const wrapper = screen.getByText('区块内容')
    expect(wrapper).toHaveClass('opacity-0')
    expect(MockIntersectionObserver.instances).toHaveLength(1)

    act(() => {
      MockIntersectionObserver.instances[0]?.emit(true)
    })

    expect(wrapper).toHaveClass('opacity-100')
    expect(wrapper).not.toHaveClass('opacity-0')
    expect(MockIntersectionObserver.instances[0]?.disconnect).toHaveBeenCalled()
  })

  it('减少动态效果时不创建 observer，也不挂过渡 class', () => {
    stubObserver()
    vi.stubGlobal(
      'matchMedia',
      vi.fn().mockReturnValue({
        matches: true,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
      }),
    )

    render(<Reveal>区块内容</Reveal>)

    const wrapper = screen.getByText('区块内容')
    expect(wrapper).toHaveClass('opacity-100')
    expect(wrapper).not.toHaveClass('motion-reveal')
    expect(MockIntersectionObserver.instances).toHaveLength(0)
  })
})
