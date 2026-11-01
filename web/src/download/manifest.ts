/**
 * 下载页读取的同源发布清单（`/downloads/latest.json`）。
 *
 * 网页不验签（add-client-releases D6）：安装包完整性由客户端验签与用户核对
 * SHA-256 保证。这里仍要做防御性解析，任何字段缺失或类型不符都视为清单不可用，
 * 绝不用不完整的数据渲染版本信息。
 */

export interface ReleaseArtifact {
  file: string
  size: number
  sha256: string
}

export interface ReleaseHistoryEntry {
  version: string
  releasedAt: string
  notes: string[]
}

export interface ReleaseManifest {
  version: string
  releasedAt: string
  minSupported: string
  notes: string[]
  history: ReleaseHistoryEntry[]
  platforms: {
    windows: ReleaseArtifact
    macos: ReleaseArtifact
  }
}

export interface ChangelogEntry {
  version: string
  releasedAt: string
  notes: string[]
}

/** D1：history 最多保存 2 个旧版本，加上当前版本共 3 个更新日志。 */
export const MAX_HISTORY = 2

const STABLE_VERSION = /^\d+\.\d+\.\d+$/
const SHA256 = /^[0-9a-f]{64}$/
const PATH_SEPARATORS = /[/\\:]/

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function stableVersion(value: unknown): string | null {
  return typeof value === 'string' && STABLE_VERSION.test(value) ? value : null
}

function timestamp(value: unknown): string | null {
  if (typeof value !== 'string' || value.length === 0) {
    return null
  }
  return Number.isNaN(Date.parse(value)) ? null : value
}

function notes(value: unknown): string[] | null {
  if (!Array.isArray(value)) {
    return null
  }
  return value.every((item) => typeof item === 'string') ? [...value] : null
}

function hasControlCharacter(value: string): boolean {
  for (const character of value) {
    const code = character.codePointAt(0) ?? 0
    if (code <= 0x1f || code === 0x7f) {
      return true
    }
  }
  return false
}

function artifact(value: unknown): ReleaseArtifact | null {
  if (!isRecord(value)) {
    return null
  }
  const { file, size, sha256 } = value
  if (
    typeof file !== 'string' ||
    file.length === 0 ||
    PATH_SEPARATORS.test(file) ||
    file.includes('..') ||
    hasControlCharacter(file)
  ) {
    return null
  }
  if (
    typeof size !== 'number' ||
    !Number.isSafeInteger(size) ||
    size <= 0
  ) {
    return null
  }
  if (typeof sha256 !== 'string' || !SHA256.test(sha256)) {
    return null
  }
  return { file, size, sha256 }
}

function historyEntry(value: unknown): ReleaseHistoryEntry | null {
  if (!isRecord(value)) {
    return null
  }
  const version = stableVersion(value.version)
  const releasedAt = timestamp(value.released_at)
  const entryNotes = notes(value.notes)
  if (!version || !releasedAt || entryNotes === null) {
    return null
  }
  return { version, releasedAt, notes: entryNotes }
}

/**
 * 把任意 JSON 值解析为发布清单；任何不符合契约的输入都返回 `null`。
 *
 * 防御性检查：
 * - schema 必须为 1，version / min_supported / history.version 必须是 X.Y.Z；
 * - released_at 必须能被解析为时间；
 * - notes 必须是字符串数组；
 * - 平台字段必须存在，size 必须是正安全整数；
 * - sha256 必须是 64 位小写十六进制；
 * - file 不能为空，且不能包含 `/`、`\`、`:`、控制字符或 `..`。
 */
export function parseReleaseManifest(value: unknown): ReleaseManifest | null {
  if (!isRecord(value) || value.schema !== 1) {
    return null
  }

  const version = stableVersion(value.version)
  const releasedAt = timestamp(value.released_at)
  const minSupported = stableVersion(value.min_supported)
  const manifestNotes = notes(value.notes)
  if (!version || !releasedAt || !minSupported || manifestNotes === null) {
    return null
  }

  if (!Array.isArray(value.history) || value.history.length > MAX_HISTORY) {
    return null
  }
  const history: ReleaseHistoryEntry[] = []
  for (const item of value.history) {
    const entry = historyEntry(item)
    if (!entry) {
      return null
    }
    history.push(entry)
  }

  if (!isRecord(value.platforms)) {
    return null
  }
  const windows = artifact(value.platforms['windows-x64'])
  const macos = artifact(value.platforms['macos-universal'])
  if (!windows || !macos) {
    return null
  }

  return {
    version,
    releasedAt,
    minSupported,
    notes: manifestNotes,
    history,
    platforms: { windows, macos },
  }
}

/** 从响应文本解析；JSON 语法错误也按清单不可用处理。 */
export function parseReleaseManifestJson(text: string): ReleaseManifest | null {
  try {
    return parseReleaseManifest(JSON.parse(text) as unknown)
  } catch {
    return null
  }
}

/**
 * 读取同源清单。调用方把任何异常都映射为“暂时无法获取最新版本”。
 * 请求不携带凭据（D6 / D5）。
 */
export async function fetchLatestManifest(
  fetcher: typeof fetch = fetch,
): Promise<ReleaseManifest> {
  const response = await fetcher('/downloads/latest.json', {
    cache: 'no-cache',
    credentials: 'omit',
  })
  if (!response.ok) {
    throw new Error('发布清单请求失败')
  }
  const body = (await response.json()) as unknown
  const manifest = parseReleaseManifest(body)
  if (!manifest) {
    throw new Error('发布清单格式无效')
  }
  return manifest
}

/** 当前版本 + 之前版本，最多 3 个；当前版本永远排在最前。 */
export function mergeChangelog(manifest: ReleaseManifest): ChangelogEntry[] {
  const entries: ChangelogEntry[] = [
    {
      version: manifest.version,
      releasedAt: manifest.releasedAt,
      notes: manifest.notes,
    },
    ...manifest.history,
  ]
  return entries.slice(0, MAX_HISTORY + 1)
}

/** 下载链接：版本目录 + URL 编码后的文件名。 */
export function downloadUrl(
  manifest: ReleaseManifest,
  platform: 'windows' | 'macos',
): string {
  const artifact =
    platform === 'windows' ? manifest.platforms.windows : manifest.platforms.macos
  return `/downloads/${manifest.version}/${encodeURIComponent(artifact.file)}`
}

/** 按本地时区显示为“2026年11月5日”。 */
export function formatReleaseDate(value: string): string {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) {
    return '—'
  }
  return new Intl.DateTimeFormat('zh-CN', {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
  }).format(date)
}

/** 安装包大小：与设计稿和桌面客户端一致，用十进制 MB（15.4 MB）。 */
export function formatInstallerSize(bytes: number): string {
  const mb = bytes / 1_000_000
  return mb >= 1000 ? `${(mb / 1000).toFixed(1)} GB` : `${mb.toFixed(1)} MB`
}
