import { describe, expect, it } from 'vitest'

import {
  entriesFromDataTransfer,
  isIgnoredName,
  scanDroppedEntries,
  scanFileList,
  type DataTransferItemLike,
  type DirectoryReaderLike,
  type FileSystemEntryLike,
} from './folder-scan'

function fileEntry(name: string, size = 3): FileSystemEntryLike {
  return {
    name,
    isFile: true,
    isDirectory: false,
    file: (success) => success(new File([new Uint8Array(size)], name)),
  }
}

function dirEntry(
  name: string,
  children: FileSystemEntryLike[],
  batchSize = 2,
): FileSystemEntryLike {
  let offset = 0
  const reader: DirectoryReaderLike = {
    readEntries: (success) => {
      const batch = children.slice(offset, offset + batchSize)
      offset += batch.length
      queueMicrotask(() => success(batch))
    },
  }
  return {
    name,
    isDirectory: true,
    isFile: false,
    createReader: () => reader,
  }
}

describe('文件夹遍历（design D1）', () => {
  it('递归还原子目录并保留空文件夹，readEntries 分批读取', async () => {
    const photo = dirEntry('照片', [
      dirEntry('2025', [fileEntry('a.jpg'), fileEntry('b.jpg')]),
      dirEntry('2026', [fileEntry('c.jpg')]),
      dirEntry('草稿', []),
    ])

    const result = await scanDroppedEntries([photo], { makeId: () => 'task' })
    expect(result.tasks).toHaveLength(1)
    const task = result.tasks[0]
    expect(task?.rootName).toBe('照片')
    expect(task?.files.map((entry) => entry.relativePath).sort()).toEqual([
      '2025/a.jpg',
      '2025/b.jpg',
      '2026/c.jpg',
    ])
    expect(task?.folders).toEqual(['2025', '2026', '草稿'])
    expect(task?.truncated).toBe(false)
    expect(result.totalBytes).toBe(9)
  })

  it('忽略系统垃圾文件且不计入失败', async () => {
    for (const name of [
      '.DS_Store',
      'Thumbs.db',
      'desktop.ini',
      '._resource',
      '~$report.docx',
      '.~lock.report.odt#',
    ]) {
      expect(isIgnoredName(name)).toBe(true)
    }
    expect(isIgnoredName('normal.txt')).toBe(false)

    const root = dirEntry('资料', [
      fileEntry('.DS_Store'),
      fileEntry('Thumbs.db'),
      fileEntry('desktop.ini'),
      fileEntry('._x'),
      fileEntry('~$x'),
      fileEntry('.~lock.x#'),
      fileEntry('keep.txt'),
    ])
    const result = await scanDroppedEntries([root], { makeId: () => 'task' })
    expect(result.tasks[0]?.files.map((entry) => entry.relativePath)).toEqual([
      'keep.txt',
    ])
    expect(result.skipped).toEqual([])
  })

  it('非法名称被跳过并列入原因，不中止其它文件', async () => {
    const longName = `${'长'.repeat(100)}.txt`
    const root = dirEntry('资料', [fileEntry(longName), fileEntry('ok.txt')])
    const result = await scanDroppedEntries([root], { makeId: () => 'task' })
    expect(result.tasks[0]?.files.map((entry) => entry.relativePath)).toEqual([
      'ok.txt',
    ])
    expect(result.skipped).toHaveLength(1)
    expect(result.skipped[0]?.reason).toContain('名称不合法')
  })

  it('单个任务超过上限时截断并给出拆分提示', async () => {
    const root = dirEntry(
      '大文件夹',
      [fileEntry('1.txt'), fileEntry('2.txt'), fileEntry('3.txt')],
      1,
    )
    const result = await scanDroppedEntries([root], {
      makeId: () => 'task',
      maxFiles: 2,
    })
    expect(result.tasks[0]?.files).toHaveLength(2)
    expect(result.tasks[0]?.truncated).toBe(true)
    expect(result.tasks[0]?.skipped[0]?.reason).toContain('最多 2 个文件')
  })

  it('拖入的散落文件合并为一个“文件”任务', async () => {
    const result = await scanDroppedEntries(
      [fileEntry('a.txt'), fileEntry('b.txt')],
      { makeId: () => 'task' },
    )
    expect(result.tasks).toHaveLength(1)
    expect(result.tasks[0]?.rootName).toBe('文件')
    expect(result.tasks[0]?.kind).toBe('files')
    expect(result.tasks[0]?.files.map((entry) => entry.relativePath)).toEqual([
      'a.txt',
      'b.txt',
    ])
  })

  it('webkitdirectory 的 FileList 按根目录分组并重建中间目录', () => {
    const make = (path: string, content = 'x', name?: string) => {
      const file = new File(
        [content],
        name ?? path.split('/').pop() ?? path,
      )
      Object.defineProperty(file, 'webkitRelativePath', { value: path })
      return file
    }
    const result = scanFileList(
      [
        make('工作资料/2025/报告/a.txt'),
        make('工作资料/根文件.txt'),
        make('照片/2025/a.jpg'),
        make('', 'x', '散落.txt'),
      ],
      { makeId: () => 'task' },
    )
    const names = result.tasks.map((task) => task.rootName).sort()
    expect(names).toEqual(['工作资料', '文件', '照片'])
    const work = result.tasks.find((task) => task.rootName === '工作资料')
    expect(work?.files.map((entry) => entry.relativePath).sort()).toEqual([
      '2025/报告/a.txt',
      '根文件.txt',
    ])
    expect(work?.folders).toEqual(['2025', '2025/报告'])
    const loose = result.tasks.find((task) => task.rootName === '文件')
    expect(loose?.files.map((entry) => entry.relativePath)).toEqual(['散落.txt'])
  })

  it('entriesFromDataTransfer 优先使用 webkitGetAsEntry，缺失时回退 getAsFile', () => {
    const entry = fileEntry('a.txt')
    const items: DataTransferItemLike[] = [
      { kind: 'file', webkitGetAsEntry: () => entry },
      {
        kind: 'file',
        getAsFile: () => new File(['fallback'], 'b.txt'),
      },
      { kind: 'string' },
    ]
    const entries = entriesFromDataTransfer(items)
    expect(entries.map((item) => item.name)).toEqual(['a.txt', 'b.txt'])
  })
})
