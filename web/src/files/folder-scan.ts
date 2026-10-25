/**
 * 文件夹选择与遍历（add-folder-backup design D1）。
 *
 * 两条入口：
 * - `<input type="file" webkitdirectory multiple>`：浏览器只给出 `FileList`，
 *   用 `webkitRelativePath` 还原目录结构；浏览器不会提供空文件夹。
 * - 拖拽：`DataTransferItem.webkitGetAsEntry()` 递归读取 `FileSystemDirectoryEntry`，
 *   `readEntries` 必须循环到返回空数组；这样可以得到空文件夹。
 *
 * 遍历在主线程分批进行，每 500 个条目让出一次事件循环，避免大目录卡住界面。
 * 忽略列表中的系统垃圾文件不进入任务，也不计入失败。
 */
import { validateName } from './name-validation'

/** 单个文件夹任务的最大文件数（D1）。 */
export const MAX_FILES_PER_TASK = 50_000

/** 每处理多少条目让出一次事件循环。 */
const YIELD_EVERY = 500

/** 系统垃圾文件（大小写不敏感；`._*`、`~$*`、`.~lock.*#` 为前缀/模式）。 */
export function isIgnoredName(name: string): boolean {
  const lower = name.toLowerCase()
  if (
    lower === '.ds_store' ||
    lower === 'thumbs.db' ||
    lower === 'desktop.ini'
  ) {
    return true
  }
  if (lower.startsWith('._') || lower.startsWith('~$')) {
    return true
  }
  return /^\.~lock\..*#$/.test(lower)
}

export interface ScanSkippedItem {
  /** 相对任务根的路径（含任务根名，便于在摘要中展示）。 */
  path: string
  reason: string
}

export interface ScannedFile {
  /** 相对任务根的 POSIX 路径（例如 `2025/a.jpg`）。 */
  relativePath: string
  file: File
}

export interface FolderScanTask {
  id: string
  /** 展示名：顶层文件夹名，或散落文件的“文件”。 */
  rootName: string
  kind: 'folder' | 'files'
  files: ScannedFile[]
  /** 相对任务根的子目录路径（含空目录）；任务根自身不在此列。 */
  folders: string[]
  skipped: ScanSkippedItem[]
  totalBytes: number
  /** 是否因为 50,000 上限被截断。 */
  truncated: boolean
}

export interface FolderScanResult {
  tasks: FolderScanTask[]
  skipped: ScanSkippedItem[]
  totalFiles: number
  totalBytes: number
}

/** `DataTransferItem` 的最小结构，便于单测伪造。 */
export interface DataTransferItemLike {
  kind: string
  webkitGetAsEntry?: () => FileSystemEntryLike | null
  getAsFile?: () => File | null
}

export interface DirectoryReaderLike {
  readEntries(
    success: (entries: FileSystemEntryLike[]) => void,
    error?: (error: unknown) => void,
  ): void
}

/** `FileSystemEntry` 的最小结构，便于单测伪造。 */
export interface FileSystemEntryLike {
  name: string
  fullPath?: string
  isFile: boolean
  isDirectory: boolean
  file?(
    success: (file: File) => void,
    error?: (error: unknown) => void,
  ): void
  createReader?(): DirectoryReaderLike
}

interface TaskBuilder {
  task: FolderScanTask
  count: number
  stopped: boolean
  maxFiles: number
}

function yieldToEventLoop(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0))
}

function joinPath(prefix: string, name: string): string {
  return prefix ? `${prefix}/${name}` : name
}

function newTask(
  id: string,
  rootName: string,
  kind: 'folder' | 'files',
  maxFiles: number = MAX_FILES_PER_TASK,
): TaskBuilder {
  return {
    task: {
      id,
      rootName,
      kind,
      files: [],
      folders: [],
      skipped: [],
      totalBytes: 0,
      truncated: false,
    },
    count: 0,
    stopped: false,
    maxFiles,
  }
}

function addFile(builder: TaskBuilder, relativePath: string, file: File): void {
  if (builder.stopped) {
    return
  }
  if (builder.count >= builder.maxFiles) {
    builder.task.truncated = true
    builder.stopped = true
    builder.task.skipped.push({
      path: builder.task.rootName,
      reason: `单个文件夹任务最多 ${builder.maxFiles.toLocaleString('zh-CN')} 个文件，请拆分后重试`,
    })
    return
  }
  builder.count += 1
  builder.task.files.push({ relativePath, file })
  builder.task.totalBytes += file.size
}

function addFolder(builder: TaskBuilder, relativePath: string): void {
  if (!relativePath) {
    return
  }
  builder.task.folders.push(relativePath)
}

function skip(builder: TaskBuilder, path: string, reason: string): void {
  builder.task.skipped.push({ path, reason })
}

function readFile(entry: FileSystemEntryLike): Promise<File> {
  return new Promise((resolve, reject) => {
    if (!entry.file) {
      reject(new Error('文件条目不可读取'))
      return
    }
    entry.file(resolve, reject)
  })
}

async function readAllEntries(
  entry: FileSystemEntryLike,
): Promise<FileSystemEntryLike[]> {
  const reader = entry.createReader?.()
  if (!reader) {
    throw new Error('目录条目不可读取')
  }
  const entries: FileSystemEntryLike[] = []
  // readEntries 一次可能只返回一批（Chrome 常见为 100 条），必须循环到空数组。
  for (;;) {
    const batch = await new Promise<FileSystemEntryLike[]>((resolve, reject) => {
      reader.readEntries(resolve, reject)
    })
    if (batch.length === 0) {
      return entries
    }
    entries.push(...batch)
  }
}

/**
 * 递归扫描拖入的顶层条目。每个顶层文件夹一个任务；散落文件合并为“文件”任务。
 */
export async function scanDroppedEntries(
  entries: FileSystemEntryLike[],
  options: {
    makeId?: () => string
    maxFiles?: number
    onProgress?: (discovered: number) => void
  } = {},
): Promise<FolderScanResult> {
  const makeId = options.makeId ?? (() => crypto.randomUUID())
  const maxFiles = options.maxFiles ?? MAX_FILES_PER_TASK
  const tasks: FolderScanTask[] = []
  const loose = newTask(makeId(), '文件', 'files', maxFiles)
  let processed = 0

  const scanDirectory = async (
    builder: TaskBuilder,
    entry: FileSystemEntryLike,
    relative: string,
  ): Promise<void> => {
    if (builder.stopped) {
      return
    }
    processed += 1
    if (processed % YIELD_EVERY === 0) {
      options.onProgress?.(processed)
      await yieldToEventLoop()
    }
    if (isIgnoredName(entry.name)) {
      return
    }
    const nameError = validateName(entry.name)
    if (nameError) {
      skip(
        builder,
        `${builder.task.rootName}/${relative}`,
        `名称不合法：${nameError}`,
      )
      return
    }
    addFolder(builder, relative)
    let children: FileSystemEntryLike[]
    try {
      children = await readAllEntries(entry)
    } catch {
      skip(builder, `${builder.task.rootName}/${relative}`, '无法读取该文件夹')
      return
    }
    for (const child of children) {
      if (builder.stopped) {
        return
      }
      const childRelative = joinPath(relative, child.name)
      if (child.isDirectory) {
        if (isIgnoredName(child.name)) {
          continue
        }
        await scanDirectory(builder, child, childRelative)
      } else if (child.isFile) {
        if (isIgnoredName(child.name)) {
          continue
        }
        const childNameError = validateName(child.name)
        if (childNameError) {
          skip(
            builder,
            `${builder.task.rootName}/${childRelative}`,
            `名称不合法：${childNameError}`,
          )
          continue
        }
        processed += 1
        if (processed % YIELD_EVERY === 0) {
          options.onProgress?.(processed)
          await yieldToEventLoop()
        }
        try {
          const file = await readFile(child)
          addFile(builder, childRelative, file)
        } catch {
          skip(
            builder,
            `${builder.task.rootName}/${childRelative}`,
            '无法读取该文件',
          )
        }
      }
    }
  }

  for (const entry of entries) {
    if (entry.isDirectory) {
      if (isIgnoredName(entry.name)) {
        continue
      }
      const rootNameError = validateName(entry.name)
      if (rootNameError) {
        loose.task.skipped.push({
          path: entry.name,
          reason: `名称不合法：${rootNameError}`,
        })
        continue
      }
      const builder = newTask(makeId(), entry.name, 'folder', maxFiles)
      await scanDirectory(builder, entry, '')
      tasks.push(builder.task)
    } else if (entry.isFile) {
      if (isIgnoredName(entry.name)) {
        continue
      }
      const nameError = validateName(entry.name)
      if (nameError) {
        skip(loose, entry.name, `名称不合法：${nameError}`)
        continue
      }
      try {
        addFile(loose, entry.name, await readFile(entry))
      } catch {
        skip(loose, entry.name, '无法读取该文件')
      }
    }
  }

  if (loose.task.files.length > 0 || loose.task.skipped.length > 0) {
    tasks.push(loose.task)
  }
  return summarize(tasks)
}

/**
 * 扫描 `<input webkitdirectory>` 的 FileList（或普通多选文件）。
 *
 * 有 `webkitRelativePath` 时按第一段分组（每组一个顶层文件夹任务）；
 * 没有时全部归入“文件”任务。
 */
export function scanFileList(
  files: File[],
  options: { makeId?: () => string; maxFiles?: number } = {},
): FolderScanResult {
  const makeId = options.makeId ?? (() => crypto.randomUUID())
  const maxFiles = options.maxFiles ?? MAX_FILES_PER_TASK
  const tasks = new Map<string, TaskBuilder>()
  let loose: TaskBuilder | null = null

  const builderFor = (rootName: string): TaskBuilder => {
    let builder = tasks.get(rootName)
    if (!builder) {
      builder = newTask(makeId(), rootName, 'folder', maxFiles)
      tasks.set(rootName, builder)
    }
    return builder
  }

  for (const file of files) {
    const raw = file.webkitRelativePath || ''
    const segments = raw.split('/').filter(Boolean)
    if (segments.length === 0) {
      loose ??= newTask(makeId(), '文件', 'files', maxFiles)
      if (isIgnoredName(file.name)) {
        continue
      }
      const nameError = validateName(file.name)
      if (nameError) {
        skip(loose, file.name, `名称不合法：${nameError}`)
        continue
      }
      addFile(loose, file.name, file)
      continue
    }
    const rootName = segments[0] ?? file.name
    if (isIgnoredName(rootName)) {
      continue
    }
    const rootNameError = validateName(rootName)
    if (rootNameError) {
      if (!loose) {
        loose = newTask(makeId(), '文件', 'files', maxFiles)
      }
      skip(loose, rootName, `名称不合法：${rootNameError}`)
      continue
    }
    const builder = builderFor(rootName)
    const relativeSegments = segments.slice(1)
    if (relativeSegments.length === 0) {
      // 理论上 webkitRelativePath 至少包含根名；防御性处理。
      continue
    }
    const fileName = relativeSegments[relativeSegments.length - 1] ?? file.name
    if (isIgnoredName(fileName)) {
      continue
    }
    let invalid: string | null = null
    for (const segment of relativeSegments) {
      invalid = validateName(segment)
      if (invalid) {
        skip(
          builder,
          `${rootName}/${relativeSegments.join('/')}`,
          `名称不合法：${invalid}`,
        )
        break
      }
    }
    if (invalid) {
      continue
    }
    for (let index = 1; index < relativeSegments.length; index += 1) {
      addFolder(builder, relativeSegments.slice(0, index).join('/'))
    }
    addFile(builder, relativeSegments.join('/'), file)
  }

  if (loose && (loose.task.files.length > 0 || loose.task.skipped.length > 0)) {
    tasks.set(loose.task.rootName, loose)
  }
  return summarize([...tasks.values()].map((builder) => builder.task))
}

function summarize(tasks: FolderScanTask[]): FolderScanResult {
  for (const task of tasks) {
    task.folders.sort()
  }
  const skipped = tasks.flatMap((task) => task.skipped)
  return {
    tasks,
    skipped,
    totalFiles: tasks.reduce((sum, task) => sum + task.files.length, 0),
    totalBytes: tasks.reduce((sum, task) => sum + task.totalBytes, 0),
  }
}

/** 从 `DataTransferItemList` 提取条目（优先 webkitGetAsEntry，可得到空目录）。 */
export function entriesFromDataTransfer(
  items: ArrayLike<DataTransferItemLike>,
): FileSystemEntryLike[] {
  const entries: FileSystemEntryLike[] = []
  for (let index = 0; index < items.length; index += 1) {
    const item = items[index]
    if (!item || item.kind !== 'file') {
      continue
    }
    const entry = item.webkitGetAsEntry?.() ?? null
    if (entry) {
      entries.push(entry)
      continue
    }
    const file = item.getAsFile?.() ?? null
    if (file) {
      entries.push({
        name: file.name,
        isFile: true,
        isDirectory: false,
        file: (success) => success(file),
      })
    }
  }
  return entries
}
