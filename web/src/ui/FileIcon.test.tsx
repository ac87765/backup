import { render } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { FileIcon } from './FileIcon'
import { FILE_VISUAL_META, fileVisualKind } from './file-visual'

describe('文件类型图标', () => {
  it('按扩展名映射到 D4 的文件类型', () => {
    expect(fileVisualKind('folders', 'folder')).toBe('folder')
    expect(fileVisualKind('a.jpg', 'file')).toBe('image')
    expect(fileVisualKind('a.PDF', 'file')).toBe('pdf')
    expect(fileVisualKind('c.zip', 'file')).toBe('archive')
    expect(fileVisualKind('movie.mp4', 'file')).toBe('video')
    expect(fileVisualKind('song.flac', 'file')).toBe('audio')
    expect(fileVisualKind('sheet.xlsx', 'file')).toBe('spreadsheet')
    expect(fileVisualKind('deck.pptx', 'file')).toBe('presentation')
    expect(fileVisualKind('notes.docx', 'file')).toBe('document')
    expect(fileVisualKind('main.ts', 'file')).toBe('code')
    expect(fileVisualKind('readme.md', 'file')).toBe('text')
    expect(fileVisualKind('no-extension', 'file')).toBe('other')
    expect(fileVisualKind('.env', 'file')).toBe('other')
  })

  it('每个类型都有中文标签与图标', () => {
    for (const meta of Object.values(FILE_VISUAL_META)) {
      expect(meta.label.length).toBeGreaterThan(0)
      expect(meta.className).toMatch(/^text-file-/)
      expect(meta.Icon).toBeTruthy()
    }
  })

  it('渲染对应类型的图标且对读屏隐藏', () => {
    const { container } = render(
      <FileIcon name="a.jpg" kind="file" size="lg" />,
    )
    const wrapper = container.querySelector('[data-file-kind="image"]')
    expect(wrapper).not.toBeNull()
    expect(wrapper).toHaveAttribute('aria-hidden', 'true')
    expect(wrapper?.querySelector('svg')).not.toBeNull()
  })
})
