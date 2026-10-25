/** 管理后台的展示辅助（不包含任何文件信息）。 */

export const GIB = 1024 * 1024 * 1024

export const QUOTA_PRESETS_GB = [5, 10, 50, 100] as const

export const STATUS_LABELS: Record<string, string> = {
  pending: '待审核',
  active: '正常',
  suspended: '已停用',
  rejected: '已拒绝',
}

export const STATUS_TONES: Record<string, string> = {
  pending: 'bg-warning-50 text-warning-600',
  active: 'bg-success-50 text-success-700',
  suspended: 'bg-danger-50 text-danger-700',
  rejected: 'bg-canvas-strong text-ink-600',
}

export function formatDateTime(value: string | null | undefined): string {
  if (!value) {
    return '—'
  }
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return '—'
  }
  return date.toLocaleString('zh-CN', { hour12: false })
}

export function bytesToGb(bytes: number): number {
  return Math.round((bytes / GIB) * 100) / 100
}

export function gbToBytes(gb: number): number {
  return Math.round(gb * GIB)
}
