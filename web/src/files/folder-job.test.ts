/* eslint-disable @typescript-eslint/prefer-promise-reject-errors -- 伪造 Worker 的普通对象错误载荷 */
import { describe, expect, it, vi } from 'vitest'

import type {
  FileNodeView,
  FolderPageView,
  TransferProgressView,
  UploadResultView,
  UsageView,
} from '../worker/types'
import { FolderUploadJob, type FolderUploadApi } from './folder-job'
import type { FolderScanTask, ScannedFile } from './folder-scan'

function file(name: string, size: number, lastModified = 1_700_000_000_000): File {
  return new File([new Uint8Array(size)], name, { lastModified })
}

function task(
  rootName: string,
  files: Array<{ path: string; file: File }>,
  folders: string[] = [],
  kind: 'folder' | 'files' = 'folder',
): FolderScanTask {
  const scanned: ScannedFile[] = files.map((entry) => ({
    relativePath: entry.path,
    file: entry.file,
  }))
  return {
    id: 'task-1',
    rootName,
    kind,
    files: scanned,
    folders,
    skipped: [],
    totalBytes: scanned.reduce((sum, entry) => sum + entry.file.size, 0),
    truncated: false,
  }
}

function node(
  id: string,
  parentId: string,
  name: string,
  kind: 'file' | 'folder',
  options: {
    size?: number
    mtime?: number
    contentSha256?: string | null
  } = {},
): FileNodeView & { contentSha256?: string | null } {
  return {
    id,
    parentId,
    kind,
    name,
    size: kind === 'file' ? (options.size ?? 0) : null,
    mtime: kind === 'file' ? (options.mtime ?? 0) : null,
    mime: kind === 'file' ? 'application/octet-stream' : null,
    hasContentSha256:
      kind === 'file' &&
      options.contentSha256 !== undefined &&
      options.contentSha256 !== null,
    rowVersion: 1,
    currentVersionId: kind === 'file' ? `version-${id}` : null,
    createdAt: '2026-01-01T00:00:00Z',
    updatedAt: '2026-01-01T00:00:00Z',
    ...(options.contentSha256 !== undefined
      ? { contentSha256: options.contentSha256 }
      : {}),
  }
}

const ROOT = 'root'

class FakeApi implements FolderUploadApi {
  nodes = new Map<string, FileNodeView>()
  createCalls: Array<{ parentId: string; name: string }> = []
  uploadCalls: Array<{ mode: 'new' | 'version'; name: string }> = []
  cancelCalls: string[] = []
  hashByName = new Map<string, string>()
  nodeHashes = new Map<string, string | null>()
  conflictOnce = new Set<string>()
  failOnce = new Set<string>()
  holdUploads = false
  pending = new Map<
    string,
    {
      complete: () => void
      reject: (error: unknown) => void
    }
  >()
  usage: UsageView = {
    usedBytes: 0,
    reservedBytes: 0,
    quotaBytes: 10 * 1024 * 1024 * 1024,
  }
  private counter = 0

  addNode(item: FileNodeView & { contentSha256?: string | null }): void {
    this.nodes.set(item.id, item)
    if (item.kind === 'file') {
      this.nodeHashes.set(item.id, item.contentSha256 ?? null)
    }
  }

  listFolder(nodeId: string): Promise<FolderPageView> {
    return Promise.resolve({
      items: [...this.nodes.values()].filter(
        (item) => item.parentId === nodeId,
      ),
      nextCursor: null,
    })
  }

  createFolder(parentId: string, name: string): Promise<FileNodeView> {
    this.createCalls.push({ parentId, name })
    if (this.conflictOnce.delete(name)) {
      const id = `race-${++this.counter}`
      const created = node(id, parentId, name, 'folder')
      this.addNode(created)
      const error = { code: 'name_conflict', message: '同名冲突' }
      return Promise.reject(error)
    }
    const existing = [...this.nodes.values()].find(
      (item) =>
        item.parentId === parentId &&
        item.kind === 'folder' &&
        item.name.toLowerCase() === name.toLowerCase(),
    )
    if (existing) {
      return Promise.reject({ code: 'name_conflict', message: '同名冲突' })
    }
    const id = `folder-${++this.counter}`
    const created = node(id, parentId, name, 'folder')
    this.addNode(created)
    return Promise.resolve(created)
  }

  private upload(
    mode: 'new' | 'version',
    parentId: string,
    fileToUpload: File,
    nodeId: string | null,
    onProgress?: (progress: TransferProgressView) => void,
  ): Promise<UploadResultView> {
    this.uploadCalls.push({ mode, name: fileToUpload.name })
    if (this.failOnce.delete(fileToUpload.name)) {
      return Promise.reject({ code: 'network_error', message: '网络中断' })
    }
    const sessionId = `session-${++this.counter}`
    onProgress?.({
      transferId: sessionId,
      direction: 'upload',
      fileName: fileToUpload.name,
      status: 'uploading',
      completedChunks: 0,
      totalChunks: 1,
      completedBytes: 0,
      totalBytes: fileToUpload.size,
    })
    return new Promise<UploadResultView>((resolve, reject) => {
      const complete = () => {
        if (mode === 'new') {
          const id = `file-${++this.counter}`
          const created = node(id, parentId, fileToUpload.name, 'file', {
            size: fileToUpload.size,
            mtime: Math.floor(fileToUpload.lastModified / 1000),
            contentSha256: this.hashByName.get(fileToUpload.name) ?? null,
          })
          this.addNode(created)
          resolve({
            nodeId: id,
            versionId: `version-${id}`,
            contentId: `content-${id}`,
            name: fileToUpload.name,
            size: fileToUpload.size,
          })
          this.pending.delete(sessionId)
          return
        }
        const targetId = nodeId ?? ''
        const target = this.nodes.get(targetId)
        if (target) {
          this.nodes.set(targetId, {
            ...target,
            size: fileToUpload.size,
            mtime: Math.floor(fileToUpload.lastModified / 1000),
            hasContentSha256: this.hashByName.has(fileToUpload.name),
            currentVersionId: `version-${targetId}-${this.counter}`,
          })
          this.nodeHashes.set(
            targetId,
            this.hashByName.get(fileToUpload.name) ?? null,
          )
        }
        resolve({
          nodeId: targetId,
          versionId: `version-${targetId}-${this.counter}`,
          contentId: `content-${targetId}`,
          name: fileToUpload.name,
          size: fileToUpload.size,
        })
        this.pending.delete(sessionId)
      }
      if (this.holdUploads) {
        this.pending.set(sessionId, { complete, reject })
      } else {
        complete()
      }
    })
  }

  uploadFile(
    parentId: string,
    fileToUpload: File,
    onProgress?: (progress: TransferProgressView) => void,
  ): Promise<UploadResultView> {
    return this.upload('new', parentId, fileToUpload, null, onProgress)
  }

  uploadNewVersion(
    nodeId: string,
    fileToUpload: File,
    onProgress?: (progress: TransferProgressView) => void,
  ): Promise<UploadResultView> {
    return this.upload('version', '', fileToUpload, nodeId, onProgress)
  }

  contentMatchesCurrentVersion(
    nodeId: string,
    fileToUpload: File,
  ): Promise<boolean | null> {
    const expected = this.nodeHashes.get(nodeId)
    if (expected === undefined || expected === null) {
      return Promise.resolve(null)
    }
    const actual =
      this.hashByName.get(fileToUpload.name) ??
      `sha-${fileToUpload.name}-${fileToUpload.size}`
    return Promise.resolve(actual === expected)
  }

  cancelUpload(sessionId: string): Promise<void> {
    this.cancelCalls.push(sessionId)
    const pending = this.pending.get(sessionId)
    pending?.reject({ code: 'cancelled', message: '已取消' })
    this.pending.delete(sessionId)
    return Promise.resolve()
  }

  getUsage(): Promise<UsageView> {
    return Promise.resolve(this.usage)
  }

  findByName(name: string): FileNodeView | undefined {
    return [...this.nodes.values()].find((item) => item.name === name)
  }
}

function run(
  api: FakeApi,
  scan: FolderScanTask,
  options: { parentId?: string; concurrency?: number } = {},
) {
  const updates: string[] = []
  const job = new FolderUploadJob({
    id: 'job-1',
    task: scan,
    parentId: options.parentId ?? ROOT,
    api,
    usage: api.usage,
    concurrency: options.concurrency ?? 3,
    onUpdate: () => updates.push(job.snapshot().status),
  })
  return { job, updates }
}

describe('FolderUploadJob（design D2/D3/D4）', () => {
  it('重建子目录结构并上传新文件', async () => {
    const api = new FakeApi()
    const scan = task(
      '照片',
      [
        { path: '2025/a.jpg', file: file('a.jpg', 3) },
        { path: '2025/b.jpg', file: file('b.jpg', 4) },
        { path: '2026/c.jpg', file: file('c.jpg', 5) },
      ],
      ['2025', '2026'],
    )
    const { job } = run(api, scan)
    await job.start()

    expect(job.snapshot().status).toBe('done')
    expect(api.createCalls.map((call) => call.name).sort()).toEqual([
      '2025',
      '2026',
      '照片',
    ])
    expect(api.uploadCalls).toHaveLength(3)
    expect(api.uploadCalls.every((call) => call.mode === 'new')).toBe(true)
    expect(job.snapshot().counts).toEqual({
      added: 3,
      updated: 0,
      unchanged: 0,
      failed: 0,
    })
    expect(api.findByName('照片')).toBeDefined()
  })

  it('第二次备份：同名文件夹合并，只更新变化文件、跳过未变化文件', async () => {
    const api = new FakeApi()
    api.addNode(node('work', ROOT, '工作资料', 'folder'))
    const files = Array.from({ length: 10 }, (_, index) => ({
      name: `f${index}.txt`,
      size: 10 + index,
    }))
    for (const [index, entry] of files.entries()) {
      api.hashByName.set(entry.name, `same-${index}`)
      api.addNode(
        node(`old-${index}`, 'work', entry.name, 'file', {
          size: entry.size,
          mtime: 1_700_000_000,
          contentSha256: `same-${index}`,
        }),
      )
    }
    // 修改 1 个（大小变化）并新增 1 个。
    const scan = task(
      '工作资料',
      [
        ...files.map((entry, index) => ({
          path: entry.name,
          file:
            index === 0
              ? file(entry.name, entry.size + 1, 1_700_000_000_000)
              : file(entry.name, entry.size, 1_700_000_000_000),
        })),
        { path: 'new.txt', file: file('new.txt', 20, 1_700_000_000_000) },
      ],
      [],
    )
    api.hashByName.set('new.txt', 'new-hash')
    const { job } = run(api, scan)
    await job.start()

    expect(job.snapshot().status).toBe('done')
    expect(job.snapshot().counts).toEqual({
      added: 1,
      updated: 1,
      unchanged: 9,
      failed: 0,
    })
    expect(api.uploadCalls).toEqual([
      { mode: 'version', name: 'f0.txt' },
      { mode: 'new', name: 'new.txt' },
    ])
    // 没有删除任何云端文件。
    expect(api.findByName('f9.txt')).toBeDefined()
  })

  it('旧版本无 content_sha256 时按大小与 mtime 回退', async () => {
    const api = new FakeApi()
    api.addNode(node('dir', ROOT, '资料', 'folder'))
    api.addNode(
      node('same', 'dir', 'same.txt', 'file', {
        size: 5,
        mtime: 1_700_000_000,
        contentSha256: null,
      }),
    )
    api.addNode(
      node('touch', 'dir', 'touch.txt', 'file', {
        size: 5,
        mtime: 1_600_000_000,
        contentSha256: null,
      }),
    )
    const scan = task(
      '资料',
      [
        { path: 'same.txt', file: file('same.txt', 5, 1_700_000_000_000) },
        { path: 'touch.txt', file: file('touch.txt', 5, 1_700_000_000_000) },
      ],
      [],
    )
    const { job } = run(api, scan)
    await job.start()

    expect(job.snapshot().counts).toEqual({
      added: 0,
      updated: 1,
      unchanged: 1,
      failed: 0,
    })
    expect(api.uploadCalls).toEqual([{ mode: 'version', name: 'touch.txt' }])
  })

  it('文件与文件夹同名冲突：跳过并在摘要中计为失败，不覆盖云端', async () => {
    const api = new FakeApi()
    api.addNode(node('cloud-report', ROOT, '报告', 'file', { size: 9 }))
    const scan = task(
      '报告',
      [{ path: 'a.txt', file: file('a.txt', 3) }],
      [],
    )
    const { job } = run(api, scan)
    await job.start()

    expect(job.snapshot().status).toBe('failed')
    expect(job.snapshot().counts.failed).toBe(1)
    expect(api.uploadCalls).toHaveLength(0)
    expect(api.findByName('报告')?.kind).toBe('file')
    expect(api.findByName('报告')?.size).toBe(9)
  })

  it('配额不足时预检失败，不创建任何目录或上传会话', async () => {
    const api = new FakeApi()
    api.usage = { usedBytes: 0, reservedBytes: 0, quotaBytes: 100 }
    const scan = task('大文件', [
      { path: 'a.bin', file: file('a.bin', 101) },
    ])
    const { job } = run(api, scan)
    await job.start()

    expect(job.snapshot().status).toBe('failed')
    expect(job.snapshot().errorCode).toBe('quota_exceeded')
    expect(job.snapshot().errorMessage).toContain('空间不足：需要')
    expect(api.createCalls).toHaveLength(0)
    expect(api.uploadCalls).toHaveLength(0)
  })

  it('超过 50,000 文件上限时不开始', async () => {
    const api = new FakeApi()
    const scan = task('超大', [])
    scan.truncated = true
    const { job } = run(api, scan)
    await job.start()
    expect(job.snapshot().status).toBe('failed')
    expect(job.snapshot().errorCode).toBe('too_many_files')
    expect(api.createCalls).toHaveLength(0)
  })

  it('name_conflict 竞态：重新列出该层并复用刚创建的文件夹', async () => {
    const api = new FakeApi()
    api.conflictOnce.add('报告')
    const scan = task('报告', [{ path: 'a.txt', file: file('a.txt', 1) }], [])
    const { job } = run(api, scan)
    await job.start()

    expect(job.snapshot().status).toBe('done')
    expect(api.uploadCalls).toEqual([{ mode: 'new', name: 'a.txt' }])
    expect(api.createCalls.filter((call) => call.name === '报告')).toHaveLength(
      1,
    )
  })

  it('重试失败项只重传失败文件，成功后失败数归零', async () => {
    const api = new FakeApi()
    api.failOnce.add('b.txt')
    const scan = task('资料', [
      { path: 'a.txt', file: file('a.txt', 1) },
      { path: 'b.txt', file: file('b.txt', 2) },
    ])
    const { job } = run(api, scan)
    await job.start()

    expect(job.snapshot().status).toBe('failed')
    expect(job.snapshot().counts).toEqual({
      added: 1,
      updated: 0,
      unchanged: 0,
      failed: 1,
    })
    expect(api.uploadCalls).toHaveLength(2)

    await job.retryFailed()
    expect(job.snapshot().status).toBe('done')
    expect(job.snapshot().counts.failed).toBe(0)
    expect(api.uploadCalls).toEqual([
      { mode: 'new', name: 'a.txt' },
      { mode: 'new', name: 'b.txt' },
      { mode: 'new', name: 'b.txt' },
    ])
  })

  it('取消：停止派发并取消进行中的上传，已完成的文件保留', async () => {
    const api = new FakeApi()
    api.usage = { usedBytes: 0, reservedBytes: 0, quotaBytes: 1024 }
    api.holdUploads = true
    const scan = task('资料', [
      { path: 'a.txt', file: file('a.txt', 1) },
      { path: 'b.txt', file: file('b.txt', 2) },
    ])
    const { job } = run(api, scan, { concurrency: 1 })
    const started = job.start()
    // 等第一个上传发出进度（拿到 sessionId）后再取消。
    for (let attempt = 0; attempt < 100 && api.pending.size === 0; attempt += 1) {
      await new Promise((resolve) => setTimeout(resolve, 1))
    }
    expect(api.pending.size).toBe(1)
    await job.cancel()
    await started

    expect(job.snapshot().status).toBe('cancelled')
    expect(api.cancelCalls).toHaveLength(1)
    // 被取消的文件没有完成，后续文件不再派发。
    expect(api.uploadCalls).toHaveLength(1)
    expect(api.findByName('a.txt')).toBeUndefined()
    expect(api.findByName('b.txt')).toBeUndefined()
  })

  it('取消：会话中止完成（预留已释放）后才显示已取消', async () => {
    const api = new FakeApi()
    api.holdUploads = true
    let releaseAbort: () => void = () => undefined
    const abortSettled = new Promise<void>((resolve) => {
      releaseAbort = resolve
    })
    const original = api.cancelUpload.bind(api)
    // 上传立即以 cancelled 结束，但服务端中止（释放预留）稍后才返回。
    vi.spyOn(api, 'cancelUpload').mockImplementation(async (sessionId) => {
      await original(sessionId)
      await abortSettled
    })
    const scan = task('资料', [{ path: 'a.txt', file: file('a.txt', 1) }])
    const { job } = run(api, scan)
    const started = job.start()
    for (let attempt = 0; attempt < 100 && api.pending.size === 0; attempt += 1) {
      await new Promise((resolve) => setTimeout(resolve, 1))
    }
    expect(api.pending.size).toBe(1)
    const cancelling = job.cancel()
    expect(api.pending.size).toBe(0)
    await new Promise((resolve) => setTimeout(resolve, 5))
    expect(job.snapshot().status).toBe('running')

    releaseAbort()
    await Promise.all([cancelling, started])
    expect(job.snapshot().status).toBe('cancelled')
    expect(api.cancelCalls).toHaveLength(1)
  })

  it('并发上传不超过设置（默认 3）', async () => {
    const api = new FakeApi()
    let active = 0
    let peak = 0
    const original = api.uploadFile.bind(api)
    vi.spyOn(api, 'uploadFile').mockImplementation(async (...args) => {
      active += 1
      peak = Math.max(peak, active)
      try {
        return await original(...args)
      } finally {
        active -= 1
      }
    })
    const scan = task(
      '多文件',
      Array.from({ length: 8 }, (_, index) => ({
        path: `f${index}.txt`,
        file: file(`f${index}.txt`, 1),
      })),
    )
    const { job } = run(api, scan, { concurrency: 3 })
    await job.start()
    expect(peak).toBeLessThanOrEqual(3)
    expect(peak).toBeGreaterThan(1)
  })
})
