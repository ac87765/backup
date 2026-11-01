import { describe, expect, it, vi } from 'vitest'

import {
  downloadUrl,
  fetchLatestManifest,
  formatReleaseDate,
  mergeChangelog,
  parseReleaseManifest,
  type ReleaseManifest,
} from './manifest'

const SHA_A = '8f3c2a91d0e74b6c5a1f9e2d7c4b8a3f6e5d0c9b8a7f6e5d4c3b2a1f0e9d8c7b'
const SHA_B = '2b7e4d1a9c8f3e6b5d0a7c4f1e8b2d9a6c3f0e7b4d1a8c5f2e9b6d3a0c7f4e1b'

function validManifest(): Record<string, unknown> {
  return {
    schema: 1,
    version: '0.2.0',
    released_at: '2026-11-05T10:00:00Z',
    min_supported: '0.1.0',
    notes: ['支持微信备份', '托盘菜单修复'],
    history: [
      {
        version: '0.1.0',
        released_at: '2026-11-01T10:00:00Z',
        notes: ['首个版本'],
      },
    ],
    platforms: {
      'windows-x64': {
        file: 'EncryptedBackup-0.2.0-windows-x64.exe',
        size: 15_400_000,
        sha256: SHA_A,
      },
      'macos-universal': {
        file: 'EncryptedBackup-0.2.0-macos.dmg',
        size: 38_500_000,
        sha256: SHA_B,
      },
    },
  }
}

function mutate(
  change: (manifest: ReturnType<typeof validManifest>) => void,
): Record<string, unknown> {
  const manifest = structuredClone(validManifest())
  change(manifest)
  return manifest
}

describe('发布清单防御性解析', () => {
  it('接受符合契约的清单并归一化字段', () => {
    const manifest = parseReleaseManifest(validManifest())

    expect(manifest).not.toBeNull()
    expect(manifest?.version).toBe('0.2.0')
    expect(manifest?.platforms.windows.file).toBe(
      'EncryptedBackup-0.2.0-windows-x64.exe',
    )
    expect(manifest?.history[0].version).toBe('0.1.0')
  })

  it('拒绝字段缺失或顶层类型不对', () => {
    expect(parseReleaseManifest(null)).toBeNull()
    expect(parseReleaseManifest('not an object')).toBeNull()
    expect(parseReleaseManifest([])).toBeNull()
    expect(parseReleaseManifest(mutate((m) => delete m.version))).toBeNull()
    expect(parseReleaseManifest(mutate((m) => delete m.platforms))).toBeNull()
    expect(
      parseReleaseManifest(
        mutate((m) => {
          const platforms = m.platforms as Record<string, unknown>
          delete platforms['windows-x64']
        }),
      ),
    ).toBeNull()
    expect(
      parseReleaseManifest(
        mutate((m) => {
          const platforms = m.platforms as Record<string, Record<string, unknown>>
          delete platforms['windows-x64'].sha256
        }),
      ),
    ).toBeNull()
  })

  it('拒绝未知 schema、非 X.Y.Z 版本或无效日期', () => {
    expect(parseReleaseManifest(mutate((m) => (m.schema = 2)))).toBeNull()
    expect(parseReleaseManifest(mutate((m) => (m.version = '0.2')))).toBeNull()
    expect(parseReleaseManifest(mutate((m) => (m.version = 'v0.2.0')))).toBeNull()
    expect(
      parseReleaseManifest(mutate((m) => (m.version = '0.2.0-dev'))),
    ).toBeNull()
    expect(
      parseReleaseManifest(mutate((m) => (m.min_supported = 'latest'))),
    ).toBeNull()
    expect(
      parseReleaseManifest(mutate((m) => (m.released_at = '昨天'))),
    ).toBeNull()
    expect(
      parseReleaseManifest(mutate((m) => (m.released_at = 123))),
    ).toBeNull()
  })

  it('拒绝类型不对的 notes 与 history', () => {
    expect(parseReleaseManifest(mutate((m) => (m.notes = '支持微信')))).toBeNull()
    expect(parseReleaseManifest(mutate((m) => (m.notes = [1])))).toBeNull()
    expect(parseReleaseManifest(mutate((m) => (m.history = {})))).toBeNull()
    expect(
      parseReleaseManifest(
        mutate((m) => {
          m.history = [
            {
              version: '0.1.0',
              released_at: '2026-11-01T10:00:00Z',
              notes: '首个版本',
            },
          ]
        }),
      ),
    ).toBeNull()
    expect(
      parseReleaseManifest(
        mutate((m) => {
          m.history = [
            {
              version: '0.1.0',
              released_at: '2026-11-01T10:00:00Z',
              notes: ['首个版本'],
            },
            {
              version: '0.0.9',
              released_at: '2026-10-01T10:00:00Z',
              notes: ['旧版本'],
            },
            {
              version: '0.0.8',
              released_at: '2026-09-01T10:00:00Z',
              notes: ['更旧版本'],
            },
          ]
        }),
      ),
    ).toBeNull()
  })

  it('拒绝类型不对的 size 与不合法的 SHA-256', () => {
    for (const size of ['15400000', 0, -1, 1.5, null]) {
      expect(
        parseReleaseManifest(
          mutate((m) => {
            const platforms = m.platforms as Record<string, Record<string, unknown>>
            platforms['windows-x64'].size = size
          }),
        ),
      ).toBeNull()
    }

    for (const sha256 of [SHA_A.toUpperCase(), SHA_A.slice(1), 'z'.repeat(64)]) {
      expect(
        parseReleaseManifest(
          mutate((m) => {
            const platforms = m.platforms as Record<string, Record<string, unknown>>
            platforms['windows-x64'].sha256 = sha256
          }),
        ),
      ).toBeNull()
    }
  })

  it('拒绝含路径分隔符、盘符、控制字符或 .. 的文件名', () => {
    for (const file of [
      '',
      'a/b.exe',
      'a\\b.exe',
      'C:evil.exe',
      '..',
      '../evil.exe',
      '..\\evil.exe',
      'evil\u0000.exe',
    ]) {
      expect(
        parseReleaseManifest(
          mutate((m) => {
            const platforms = m.platforms as Record<string, Record<string, unknown>>
            platforms['windows-x64'].file = file
          }),
        ),
      ).toBeNull()
    }
  })
})

describe('下载链接与更新日志', () => {
  it('下载链接使用版本目录并对文件名做 encodeURIComponent', () => {
    const manifest = parseReleaseManifest(validManifest()) as ReleaseManifest
    manifest.platforms.windows.file = 'Encrypted Backup-0.2.0.exe'

    expect(downloadUrl(manifest, 'windows')).toBe(
      '/downloads/0.2.0/Encrypted%20Backup-0.2.0.exe',
    )
    expect(downloadUrl(manifest, 'macos')).toBe(
      '/downloads/0.2.0/EncryptedBackup-0.2.0-macos.dmg',
    )
  })

  it('当前版本在前，合并 history 后最多 3 个版本', () => {
    const manifest = parseReleaseManifest(validManifest()) as ReleaseManifest
    const merged = mergeChangelog(manifest)
    expect(merged.map((entry) => entry.version)).toEqual(['0.2.0', '0.1.0'])

    manifest.history = [
      ...manifest.history,
      {
        version: '0.0.9',
        releasedAt: '2026-10-01T10:00:00Z',
        notes: ['旧版本'],
      },
      {
        version: '0.0.8',
        releasedAt: '2026-09-01T10:00:00Z',
        notes: ['更旧版本'],
      },
    ]
    expect(mergeChangelog(manifest).map((entry) => entry.version)).toEqual([
      '0.2.0',
      '0.1.0',
      '0.0.9',
    ])
  })

  it('发布日期本地化为中文年月日', () => {
    expect(formatReleaseDate('2026-11-05T10:00:00Z')).toBe('2026年11月5日')
    expect(formatReleaseDate('not-a-date')).toBe('—')
  })
})

describe('fetchLatestManifest', () => {
  it('使用同源、no-cache、不携带凭据的请求', async () => {
    const fetcher = vi.fn().mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(validManifest()),
    })

    const manifest = await fetchLatestManifest(fetcher)

    expect(fetcher).toHaveBeenCalledWith('/downloads/latest.json', {
      cache: 'no-cache',
      credentials: 'omit',
    })
    expect(manifest.version).toBe('0.2.0')
  })

  it('HTTP 错误或格式无效时抛出，由页面显示不可用状态', async () => {
    const httpError = vi.fn().mockResolvedValue({
      ok: false,
      json: () => Promise.resolve(validManifest()),
    })
    await expect(
      fetchLatestManifest(httpError as unknown as typeof fetch),
    ).rejects.toThrow()

    const invalid = vi.fn().mockResolvedValue({
      ok: true,
      json: () => Promise.resolve(mutate((m) => (m.version = 'nope'))),
    })
    await expect(
      fetchLatestManifest(invalid as unknown as typeof fetch),
    ).rejects.toThrow()
  })
})
