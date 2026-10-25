/**
 * 文件夹上传编排（add-folder-backup design D2/D4/D6）。
 *
 * 主线程只做编排与进度展示：遍历与加密/哈希仍在 Worker 内。
 * 流程：预检（配额/文件数）→ 计划（逐层合并目录、增量判定）→ 上传（文件级并发 3）。
 *
 * 安全边界：本模块只接触解密后的名称/大小/时间、明文 SHA-256 与文件对象；
 * 不接触任何密钥、包裹密钥或密文。
 */
import type {
  FileNodeView,
  FolderPageView,
  TransferProgressView,
  UploadResultView,
  UsageView,
} from '../worker/types'
import { formatBytes } from './format'
import {
  MAX_FILES_PER_TASK,
  type FolderScanTask,
  type ScannedFile,
} from './folder-scan'

/** 与 crypto-core/transfer-client 一致：4 MiB 明文 + 每块 41 字节帧开销。 */
const CHUNK_PLAINTEXT_SIZE = 4 * 1024 * 1024
const CHUNK_FRAME_OVERHEAD = 41

function projectedSize(size: number): number {
  return size + Math.max(1, Math.ceil(size / CHUNK_PLAINTEXT_SIZE)) * CHUNK_FRAME_OVERHEAD
}

export type FolderJobStatus =
  | 'scanning'
  | 'planning'
  | 'running'
  | 'done'
  | 'cancelled'
  | 'failed'

export type FolderChildStatus =
  | 'pending'
  | 'uploading'
  | 'done'
  | 'skipped'
  | 'failed'

export interface FolderJobCounts {
  added: number
  updated: number
  unchanged: number
  failed: number
}

export interface FolderChildView {
  id: string
  name: string
  relativePath: string
  status: FolderChildStatus
  error?: string
}

export interface FolderJobView {
  id: string
  name: string
  kind: 'folder' | 'files'
  direction: 'upload' | 'download'
  status: FolderJobStatus
  doneFiles: number
  totalFiles: number
  completedBytes: number
  totalBytes: number
  counts: FolderJobCounts
  /** 最多 200 行；更多时由 hiddenChildren 表示其余数量。 */
  children: FolderChildView[]
  hiddenChildren: number
  retrying: boolean
  /** 是否存在可重试的网络失败（同名冲突不可重试）。 */
  retryable: boolean
  errorCode?: string
  errorMessage?: string
}

/** FolderUploadJob 需要的 Worker API 子集（单测可伪造）。 */
export interface FolderUploadApi {
  listFolder(nodeId: string, cursor?: string): Promise<FolderPageView>
  createFolder(parentId: string, name: string): Promise<FileNodeView>
  uploadFile(
    parentId: string,
    file: File,
    onProgress?: (progress: TransferProgressView) => void,
  ): Promise<UploadResultView>
  uploadNewVersion(
    nodeId: string,
    file: File,
    onProgress?: (progress: TransferProgressView) => void,
  ): Promise<UploadResultView>
  /** true=未变化，false=变化，null=云端没有摘要（回退 mtime）。 */
  contentMatchesCurrentVersion(
    nodeId: string,
    file: File,
  ): Promise<boolean | null>
  cancelUpload(sessionId: string): Promise<void>
  getUsage(): Promise<UsageView>
}

export interface FolderUploadJobOptions {
  id?: string
  task: FolderScanTask
  parentId: string
  api: FolderUploadApi
  usage?: UsageView
  concurrency?: number
  onUpdate?: (job: FolderJobView) => void
}

interface UploadPlanItem {
  childId: string
  mode: 'new' | 'version'
  parentId: string
  nodeId: string | null
  file: File
  relativePath: string
}

interface PlanResult {
  uploads: UploadPlanItem[]
  counts: FolderJobCounts
}

function normalizeName(name: string): string {
  return name.normalize('NFC').toLowerCase().normalize('NFC')
}

function mtimeSeconds(file: File): number {
  return Math.floor(file.lastModified / 1000)
}

function errorCode(error: unknown): string | undefined {
  const candidate = error as { code?: unknown } | null
  return candidate && typeof candidate.code === 'string'
    ? candidate.code
    : undefined
}

function errorMessage(error: unknown): string {
  const candidate = error as { message?: unknown } | null
  if (candidate && typeof candidate.message === 'string') {
    return candidate.message
  }
  return '操作失败'
}

interface LocalFolder {
  name: string
  path: string
  folders: Map<string, LocalFolder>
  files: ScannedFile[]
}

function buildLocalTree(task: FolderScanTask): LocalFolder {
  const root: LocalFolder = { name: task.rootName, path: '', folders: new Map(), files: [] }
  for (const folderPath of task.folders) {
    const segments = folderPath.split('/')
    let current = root
    for (const segment of segments) {
      let next = current.folders.get(segment)
      if (!next) {
        next = {
          name: segment,
          path: current.path ? `${current.path}/${segment}` : segment,
          folders: new Map(),
          files: [],
        }
        current.folders.set(segment, next)
      }
      current = next
    }
  }
  for (const file of task.files) {
    const segments = file.relativePath.split('/')
    segments.pop()
    let current = root
    for (const segment of segments) {
      let next = current.folders.get(segment)
      if (!next) {
        next = {
          name: segment,
          path: current.path ? `${current.path}/${segment}` : segment,
          folders: new Map(),
          files: [],
        }
        current.folders.set(segment, next)
      }
      current = next
    }
    current.files.push(file)
  }
  return root
}

async function listAll(
  api: FolderUploadApi,
  folderId: string,
): Promise<FileNodeView[]> {
  const items: FileNodeView[] = []
  let cursor: string | undefined
  for (;;) {
    const page = await api.listFolder(folderId, cursor)
    items.push(...page.items)
    if (!page.nextCursor) {
      return items
    }
    cursor = page.nextCursor
  }
}

export class FolderUploadJob {
  readonly id: string
  private readonly task: FolderScanTask
  private readonly parentId: string
  private readonly api: FolderUploadApi
  private readonly concurrency: number
  private readonly onUpdate: ((job: FolderJobView) => void) | undefined
  private providedUsage: UsageView | undefined

  private status: FolderJobStatus = 'planning'
  private counts: FolderJobCounts = {
    added: 0,
    updated: 0,
    unchanged: 0,
    failed: 0,
  }
  private children = new Map<string, FolderChildView>()
  private doneFiles = 0
  private totalFiles: number
  private totalBytes: number
  private finishedBytes = 0
  private activeBytes = new Map<string, number>()
  private activeSessions = new Map<string, string>()
  private cancelled = false
  private retrying = false
  private errorCodeValue: string | undefined
  private errorMessageValue: string | undefined
  /** 失败文件可重试：保留计划项与原因。 */
  private failedItems: Array<{ plan: UploadPlanItem; error: string }> = []

  constructor(options: FolderUploadJobOptions) {
    this.id = options.id ?? crypto.randomUUID()
    this.task = options.task
    this.parentId = options.parentId
    this.api = options.api
    this.providedUsage = options.usage
    this.concurrency = Math.max(1, options.concurrency ?? 3)
    this.onUpdate = options.onUpdate
    this.totalFiles = options.task.files.length
    this.totalBytes = options.task.totalBytes
    for (const file of options.task.files) {
      this.children.set(file.relativePath, {
        id: file.relativePath,
        name: file.file.name,
        relativePath: file.relativePath,
        status: 'pending',
      })
    }
  }

  snapshot(): FolderJobView {
    const all = [...this.children.values()]
    const priority: Record<FolderChildStatus, number> = {
      failed: 0,
      uploading: 1,
      pending: 2,
      skipped: 3,
      done: 4,
    }
    all.sort((left, right) => priority[left.status] - priority[right.status])
    const children = all.slice(0, 200)
    return {
      id: this.id,
      name: this.task.rootName,
      kind: this.task.kind,
      direction: 'upload',
      status: this.status,
      doneFiles: this.doneFiles,
      totalFiles: this.totalFiles,
      completedBytes: this.computeCompletedBytes(),
      totalBytes: this.totalBytes,
      counts: { ...this.counts },
      children,
      hiddenChildren: Math.max(0, all.length - children.length),
      retrying: this.retrying,
      retryable: this.failedItems.length > 0,
      ...(this.errorCodeValue ? { errorCode: this.errorCodeValue } : {}),
      ...(this.errorMessageValue
        ? { errorMessage: this.errorMessageValue }
        : {}),
    }
  }

  private emit(): void {
    this.onUpdate?.(this.snapshot())
  }

  private fail(code: string, message: string): void {
    this.status = 'failed'
    this.errorCodeValue = code
    this.errorMessageValue = message
    this.emit()
  }

  /** 开始任务；预检不通过时不会创建任何目录或上传会话。 */
  async start(): Promise<void> {
    this.status = 'planning'
    this.emit()
    if (this.task.truncated) {
      this.fail(
        'too_many_files',
        `这个文件夹超过 ${MAX_FILES_PER_TASK.toLocaleString('zh-CN')} 个文件，请拆分成几个文件夹后再备份。`,
      )
      return
    }

    // 预检：比较全部待上传文件的密文大小与剩余配额。目录创建在预检之后。
    let usage = this.providedUsage
    if (!usage) {
      try {
        usage = await this.api.getUsage()
      } catch (error) {
        this.fail('usage_unavailable', `无法读取剩余空间：${errorMessage(error)}`)
        return
      }
    }
    const remaining = Math.max(
      0,
      usage.quotaBytes - usage.usedBytes - usage.reservedBytes,
    )
    const needed = this.task.files.reduce(
      (sum, entry) => sum + projectedSize(entry.file.size),
      0,
    )
    if (needed > remaining) {
      this.fail(
        'quota_exceeded',
        `空间不足：需要 ${formatBytes(needed)}，剩余 ${formatBytes(remaining)}。请清理一些文件或拆小文件夹后再试。`,
      )
      return
    }

    let plan: PlanResult
    try {
      plan = await this.plan()
    } catch (error) {
      this.fail(
        errorCode(error) ?? 'plan_failed',
        `准备文件夹时失败：${errorMessage(error)}`,
      )
      return
    }
    if (this.cancelled) {
      this.finishCancelled()
      return
    }

    this.counts = plan.counts
    this.doneFiles = plan.counts.unchanged + plan.counts.failed
    this.finishedBytes = this.task.files
      .filter((entry) => {
        const child = this.children.get(entry.relativePath)
        return child?.status === 'skipped' || child?.status === 'failed'
      })
      .reduce((sum, entry) => sum + entry.file.size, 0)
    this.emit()

    this.status = 'running'
    this.emit()
    await this.runUploads(plan.uploads)

    if (this.cancelled) {
      this.finishCancelled()
      return
    }
    this.status = this.counts.failed > 0 ? 'failed' : 'done'
    this.retrying = false
    this.emit()
  }

  /** 取消：不再派发新文件，取消进行中的上传（中止会话即释放配额预留）。 */
  async cancel(): Promise<void> {
    if (this.status === 'done' || this.status === 'cancelled') {
      return
    }
    this.cancelled = true
    const sessions = [...this.activeSessions.values()]
    await Promise.all(
      sessions.map(async (sessionId) => {
        try {
          await this.api.cancelUpload(sessionId)
        } catch {
          // 会话可能已提交/关闭；本地仍按取消处理。
        }
      }),
    )
    this.emit()
  }

  /** 只重试失败项；成功后摘要中的失败数归零。 */
  async retryFailed(): Promise<void> {
    if (this.failedItems.length === 0 || this.status === 'running') {
      return
    }
    const retryItems = this.failedItems
    this.failedItems = []
    this.counts.failed = Math.max(0, this.counts.failed - retryItems.length)
    this.doneFiles = Math.max(0, this.doneFiles - retryItems.length)
    this.cancelled = false
    this.retrying = true
    this.status = 'running'
    this.errorCodeValue = undefined
    this.errorMessageValue = undefined
    for (const item of retryItems) {
      const child = this.children.get(item.plan.relativePath)
      if (child) {
        child.status = 'pending'
        delete child.error
      }
    }
    this.emit()
    await this.runUploads(retryItems.map((item) => item.plan))
    if (this.cancelled) {
      this.finishCancelled()
      return
    }
    this.retrying = false
    this.status = this.counts.failed > 0 ? 'failed' : 'done'
    this.emit()
  }

  private finishCancelled(): void {
    this.status = 'cancelled'
    this.retrying = false
    for (const child of this.children.values()) {
      if (child.status === 'uploading' || child.status === 'pending') {
        child.status = 'skipped'
        child.error = '已取消'
      }
    }
    this.emit()
  }

  private async plan(): Promise<PlanResult> {
    const root = buildLocalTree(this.task)
    const result: PlanResult = {
      uploads: [],
      counts: { added: 0, updated: 0, unchanged: 0, failed: 0 },
    }
    const conflict = (file: ScannedFile, reason: string) => {
      const child = this.children.get(file.relativePath)
      if (child) {
        child.status = 'failed'
        child.error = reason
      }
      result.counts.failed += 1
    }
    const markUnchanged = (file: ScannedFile) => {
      const child = this.children.get(file.relativePath)
      if (child) {
        child.status = 'skipped'
      }
      result.counts.unchanged += 1
    }

    const merge = async (
      local: LocalFolder,
      cloudId: string,
    ): Promise<void> => {
      if (this.cancelled) {
        return
      }
      const cloudItems = await listAll(this.api, cloudId)
      const byName = new Map<string, FileNodeView>()
      for (const item of cloudItems) {
        byName.set(normalizeName(item.name), item)
      }

      for (const localFolder of local.folders.values()) {
        if (this.cancelled) {
          return
        }
        const existing = byName.get(normalizeName(localFolder.name))
        if (existing && existing.kind === 'file') {
          for (const file of localFolder.files) {
            conflict(file, `云端同一位置已有同名文件“${existing.name}”`)
          }
          for (const nested of localFolder.folders.values()) {
            for (const file of collectFiles(nested)) {
              conflict(file, `云端同一位置已有同名文件“${existing.name}”`)
            }
          }
          continue
        }
        let folderId: string
        if (existing && existing.kind === 'folder') {
          folderId = existing.id
        } else {
          folderId = await this.createFolderWithRaceRetry(
            cloudId,
            localFolder.name,
          )
        }
        await merge(localFolder, folderId)
      }

      for (const file of local.files) {
        if (this.cancelled) {
          return
        }
        const existing = byName.get(normalizeName(file.file.name))
        if (!existing) {
          this.addUpload(result, {
            childId: file.relativePath,
            mode: 'new',
            parentId: cloudId,
            nodeId: null,
            file: file.file,
            relativePath: file.relativePath,
          })
          continue
        }
        if (existing.kind === 'folder') {
          conflict(
            file,
            `云端同一位置已有同名文件夹“${existing.name}”`,
          )
          continue
        }
        if (existing.size !== file.file.size) {
          this.addUpload(result, {
            childId: file.relativePath,
            mode: 'version',
            parentId: cloudId,
            nodeId: existing.id,
            file: file.file,
            relativePath: file.relativePath,
          })
          continue
        }
        // 大小相同：有摘要则由 Worker 流式哈希并常数时间比较；旧版本回退到 mtime。
        let changed: boolean
        try {
          if (existing.hasContentSha256) {
            const matches = await this.api.contentMatchesCurrentVersion(
              existing.id,
              file.file,
            )
            changed =
              matches === null
                ? existing.mtime !== mtimeSeconds(file.file)
                : !matches
          } else {
            changed = existing.mtime !== mtimeSeconds(file.file)
          }
        } catch (error) {
          conflict(file, `无法读取文件内容：${errorMessage(error)}`)
          continue
        }
        if (changed) {
          this.addUpload(result, {
            childId: file.relativePath,
            mode: 'version',
            parentId: cloudId,
            nodeId: existing.id,
            file: file.file,
            relativePath: file.relativePath,
          })
        } else {
          markUnchanged(file)
        }
      }
    }

    // 任务根目录：同名文件夹合并；同名文件冲突。
    if (this.task.kind === 'folder') {
      const existing = (
        await listAll(this.api, this.parentId)
      ).find(
        (item) => normalizeName(item.name) === normalizeName(this.task.rootName),
      )
      if (existing && existing.kind === 'file') {
        for (const file of this.task.files) {
          conflict(file, `云端同一位置已有同名文件“${existing.name}”`)
        }
        return result
      }
      const rootId =
        existing && existing.kind === 'folder'
          ? existing.id
          : await this.createFolderWithRaceRetry(
              this.parentId,
              this.task.rootName,
            )
      await merge(root, rootId)
    } else {
      await merge(root, this.parentId)
    }
    return result
  }

  private addUpload(result: PlanResult, plan: UploadPlanItem): void {
    result.uploads.push(plan)
    const child = this.children.get(plan.relativePath)
    if (child) {
      child.status = 'pending'
    }
  }

  private async createFolderWithRaceRetry(
    parentId: string,
    name: string,
  ): Promise<string> {
    try {
      const created = await this.api.createFolder(parentId, name)
      return created.id
    } catch (error) {
      if (errorCode(error) !== 'name_conflict') {
        throw error
      }
      const items = await listAll(this.api, parentId)
      const existing = items.find(
        (item) =>
          normalizeName(item.name) === normalizeName(name) &&
          item.kind === 'folder',
      )
      if (existing) {
        return existing.id
      }
      throw error
    }
  }

  private async runUploads(uploads: UploadPlanItem[]): Promise<void> {
    let next = 0
    const workerCount = Math.min(this.concurrency, uploads.length)
    const workers = Array.from({ length: workerCount }, async () => {
      for (;;) {
        if (this.cancelled) {
          return
        }
        const index = next
        next += 1
        if (index >= uploads.length) {
          return
        }
        await this.uploadOne(uploads[index])
      }
    })
    await Promise.all(workers)
  }

  private async uploadOne(plan: UploadPlanItem): Promise<void> {
    const child = this.children.get(plan.relativePath)
    if (child) {
      child.status = 'uploading'
      delete child.error
    }
    this.emit()
    const progress = (event: TransferProgressView) => {
      if (event.transferId) {
        this.activeSessions.set(plan.childId, event.transferId)
        if (this.cancelled) {
          void this.api.cancelUpload(event.transferId).catch(() => undefined)
        }
        if (event.status === 'retrying') {
          this.retrying = true
        } else {
          this.retrying = false
        }
      }
      this.activeBytes.set(plan.childId, event.completedBytes)
      this.emit()
    }
    try {
      if (plan.mode === 'new') {
        await this.api.uploadFile(plan.parentId, plan.file, progress)
      } else {
        await this.api.uploadNewVersion(plan.nodeId ?? '', plan.file, progress)
      }
      if (this.cancelled) {
        if (child) {
          child.status = 'skipped'
          child.error = '已取消'
        }
        return
      }
      if (child) {
        child.status = 'done'
      }
      if (plan.mode === 'new') {
        this.counts.added += 1
      } else {
        this.counts.updated += 1
      }
      this.doneFiles += 1
      this.finishedBytes += plan.file.size
      this.activeBytes.delete(plan.childId)
      this.activeSessions.delete(plan.childId)
      this.emit()
    } catch (error) {
      if (this.cancelled || errorCode(error) === 'cancelled') {
        if (child) {
          child.status = 'skipped'
          child.error = '已取消'
        }
        return
      }
      const message = errorMessage(error)
      if (child) {
        child.status = 'failed'
        child.error = message
      }
      this.counts.failed += 1
      this.doneFiles += 1
      this.failedItems.push({ plan, error: message })
      this.activeBytes.delete(plan.childId)
      this.activeSessions.delete(plan.childId)
      this.emit()
    }
  }

  private computeCompletedBytes(): number {
    let bytes = this.finishedBytes
    for (const value of this.activeBytes.values()) {
      bytes += value
    }
    return Math.min(this.totalBytes, Math.max(0, bytes))
  }
}

function collectFiles(folder: LocalFolder): ScannedFile[] {
  const files = [...folder.files]
  for (const nested of folder.folders.values()) {
    files.push(...collectFiles(nested))
  }
  return files
}
