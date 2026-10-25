import { describe, expect, it } from 'vitest'

import type { FileNodeView } from '../worker/types'
import {
  filterFileNodes,
  nextSort,
  sortFileNodes,
  type SortState,
} from './view-preferences'

function node(overrides: Partial<FileNodeView>): FileNodeView {
  return {
    id: overrides.id ?? overrides.name ?? 'id',
    parentId: 'root',
    kind: 'file',
    name: 'file',
    size: 10,
    mtime: 100,
    mime: 'text/plain',
    rowVersion: 1,
    currentVersionId: 'version',
    createdAt: '2026-01-01T00:00:00Z',
    updatedAt: '2026-01-01T00:00:00Z',
    ...overrides,
  }
}

const ITEMS = [
  node({ name: 'b.txt', size: 30, mtime: 300 }),
  node({ name: '文件夹', kind: 'folder', size: null, mtime: null }),
  node({ name: 'a.txt', size: 10, mtime: 100 }),
  node({ name: 'c.txt', size: 20, mtime: 200 }),
]

describe('文件排序与筛选', () => {
  it('文件夹始终排在文件前，名称升序', () => {
    const sorted = sortFileNodes(ITEMS, { field: 'name', direction: 'asc' })
    expect(sorted.map((item) => item.name)).toEqual([
      '文件夹',
      'a.txt',
      'b.txt',
      'c.txt',
    ])
  })

  it('大小列首次升序、再次点击降序', () => {
    const asc = nextSort({ field: 'name', direction: 'asc' }, 'size')
    expect(asc).toEqual({ field: 'size', direction: 'asc' })
    expect(
      sortFileNodes(ITEMS, asc).map((item) => item.name),
    ).toEqual(['文件夹', 'a.txt', 'c.txt', 'b.txt'])

    const desc = nextSort(asc, 'size')
    expect(desc).toEqual({ field: 'size', direction: 'desc' })
    expect(
      sortFileNodes(ITEMS, desc).map((item) => item.name),
    ).toEqual(['文件夹', 'b.txt', 'c.txt', 'a.txt'])
  })

  it('修改时间排序支持升降序', () => {
    const sort: SortState = { field: 'mtime', direction: 'desc' }
    expect(sortFileNodes(ITEMS, sort).map((item) => item.name)).toEqual([
      '文件夹',
      'b.txt',
      'c.txt',
      'a.txt',
    ])
  })

  it('按名称本地过滤，不区分大小写', () => {
    const items = [
      node({ name: '合同.pdf' }),
      node({ name: '合同附件.docx' }),
      node({ name: '发票.pdf' }),
    ]
    expect(filterFileNodes(items, '合同').map((item) => item.name)).toEqual([
      '合同.pdf',
      '合同附件.docx',
    ])
    expect(filterFileNodes(items, '')).toHaveLength(3)
  })
})
