import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { KeyPair } from './KeyPair'
import { SecurityGlyph, type SecurityGlyphKind } from './SecurityGlyph'

const KINDS: SecurityGlyphKind[] = [
  'eye',
  'lock',
  'code',
  'device',
  'desktop',
  'paper',
  'copies',
  'vault',
  'no-share',
  'rotate',
  'checklist',
]

describe('安全页小插图', () => {
  it('全部小图标使用 1.75 线宽并默认对读屏隐藏', () => {
    const { container } = render(
      <div>
        {KINDS.map((kind) => (
          <SecurityGlyph key={kind} kind={kind} />
        ))}
      </div>,
    )

    const icons = container.querySelectorAll('svg')
    expect(icons).toHaveLength(KINDS.length)
    for (const icon of icons) {
      expect(icon).toHaveAttribute('stroke-width', '1.75')
      expect(icon).toHaveAttribute('aria-hidden', 'true')
    }
  })

  it('带 title 的小图标作为有含义的图形暴露', () => {
    render(<SecurityGlyph kind="lock" title="服务器看不到" />)

    expect(
      screen.getByRole('img', { name: '服务器看不到' }),
    ).toBeInTheDocument()
  })

  it('两把钥匙按状态显示完整或丢失', () => {
    const { container } = render(
      <KeyPair password="lost" recovery="present" />,
    )

    expect(
      screen.getByRole('img', {
        name: '两把钥匙：密码钥匙已丢失，恢复密钥完好',
      }),
    ).toBeVisible()
    expect(container.querySelector('[data-state="lost"]')).not.toBeNull()
    expect(container.querySelector('[data-state="present"]')).not.toBeNull()
    expect(
      container.querySelector('[data-state="lost"] g'),
    ).toHaveAttribute('stroke-dasharray', '3 3')
    expect(
      container.querySelector('[data-state="present"] g'),
    ).not.toHaveAttribute('stroke-dasharray')
    expect(container.querySelectorAll('svg')).toHaveLength(1)
  })
})
