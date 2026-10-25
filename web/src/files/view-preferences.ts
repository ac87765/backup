import type { FileNodeView } from '../worker/types'
import { nameCollator } from './format'

/**
 * 只允许写入这两个 localStorage 键（design D6 / web-ui spec）。
 * 不存文件名、密钥、加密元数据或任何其他状态。
 */
export const VIEW_MODE_STORAGE_KEY = 'backup.viewMode'
export const SORT_STORAGE_KEY = 'backup.sort'

export type ViewMode = 'list' | 'grid'
export type SortField = 'name' | 'mtime' | 'size'
export type SortDirection = 'asc' | 'desc'

export interface SortState {
  field: SortField
  direction: SortDirection
}

export const DEFAULT_SORT: SortState = { field: 'name', direction: 'asc' }

const SORT_FIELDS = new Set<SortField>(['name', 'mtime', 'size'])

function readStorage(key: string): string | null {
  try {
    return window.localStorage.getItem(key)
  } catch {
    return null
  }
}

function writeStorage(key: string, value: string): void {
  try {
    window.localStorage.setItem(key, value)
  } catch {
    // 隐私模式或存储被禁用时静默降级为仅当前会话生效。
  }
}

export function loadViewMode(): ViewMode {
  return readStorage(VIEW_MODE_STORAGE_KEY) === 'grid' ? 'grid' : 'list'
}

export function saveViewMode(mode: ViewMode): void {
  writeStorage(VIEW_MODE_STORAGE_KEY, mode)
}

export function loadSort(): SortState {
  const raw = readStorage(SORT_STORAGE_KEY)
  if (!raw) {
    return DEFAULT_SORT
  }
  try {
    const parsed = JSON.parse(raw) as {
      field?: unknown
      direction?: unknown
    }
    const field = parsed.field
    const direction = parsed.direction
    if (
      typeof field === 'string' &&
      SORT_FIELDS.has(field as SortField) &&
      (direction === 'asc' || direction === 'desc')
    ) {
      return { field: field as SortField, direction }
    }
  } catch {
    // 损坏的值回退到默认排序。
  }
  return DEFAULT_SORT
}

export function saveSort(sort: SortState): void {
  writeStorage(SORT_STORAGE_KEY, JSON.stringify(sort))
}

function directionFactor(direction: SortDirection): number {
  return direction === 'asc' ? 1 : -1
}

function compareByName(left: FileNodeView, right: FileNodeView): number {
  return nameCollator.compare(left.name, right.name)
}

/** 文件夹始终排在文件之前；同一组内按当前列排序，名称作为稳定的次级排序。 */
export function sortFileNodes(
  items: FileNodeView[],
  sort: SortState,
): FileNodeView[] {
  const factor = directionFactor(sort.direction)
  const compare = (left: FileNodeView, right: FileNodeView): number => {
    if (left.kind !== right.kind) {
      return left.kind === 'folder' ? -1 : 1
    }
    if (left.kind === 'folder' && sort.field !== 'name') {
      return compareByName(left, right)
    }
    const primary =
      sort.field === 'name'
        ? compareByName(left, right)
        : sort.field === 'mtime'
          ? (left.mtime ?? 0) - (right.mtime ?? 0)
          : (left.size ?? 0) - (right.size ?? 0)
    if (primary !== 0) {
      return primary * factor
    }
    return compareByName(left, right)
  }
  return [...items].sort(compare)
}

/** 点击同一列切换方向；切换到新列时从升序开始。 */
export function nextSort(current: SortState, field: SortField): SortState {
  if (current.field === field) {
    return {
      field,
      direction: current.direction === 'asc' ? 'desc' : 'asc',
    }
  }
  return { field, direction: 'asc' }
}

/** 当前文件夹内的本地筛选（不区分大小写，不发请求）。 */
export function filterFileNodes(
  items: FileNodeView[],
  query: string,
): FileNodeView[] {
  const needle = query.trim().toLocaleLowerCase('zh-CN')
  if (!needle) {
    return items
  }
  return items.filter((item) =>
    item.name.toLocaleLowerCase('zh-CN').includes(needle),
  )
}
