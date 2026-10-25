/** 文件浏览器使用的格式化工具。 */

export const nameCollator = new Intl.Collator('zh-Hans', {
  numeric: true,
  sensitivity: 'base',
})

export function formatBytes(bytes: number | null): string {
  if (bytes === null) {
    return '—'
  }
  if (bytes < 1024) {
    return `${bytes} B`
  }
  const units = ['KiB', 'MiB', 'GiB', 'TiB']
  let value = bytes / 1024
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unit]}`
}

export function formatTimestamp(value: string | null): string {
  if (!value) {
    return '—'
  }
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return '—'
  }
  return date.toLocaleString('zh-CN', { hour12: false })
}

export function formatUnixTime(seconds: number | null): string {
  if (seconds === null) {
    return '—'
  }
  return formatTimestamp(new Date(seconds * 1000).toISOString())
}

export function sortByName<T extends { name: string }>(items: T[]): T[] {
  return [...items].sort((left, right) => nameCollator.compare(left.name, right.name))
}
