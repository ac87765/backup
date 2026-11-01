import { render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, describe, expect, it, vi } from 'vitest'

import App from '../App'
import { DownloadPage } from './DownloadPage'

const SHA_WINDOWS =
  '8f3c2a91d0e74b6c5a1f9e2d7c4b8a3f6e5d0c9b8a7f6e5d4c3b2a1f0e9d8c7b'
const SHA_MACOS =
  '2b7e4d1a9c8f3e6b5d0a7c4f1e8b2d9a6c3f0e7b4d1a8c5f2e9b6d3a0c7f4e1b'

const WINDOWS_UA =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36'
const MAC_UA =
  'Mozilla/5.0 (Macintosh; Intel Mac OS X 14_5) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Safari/605.1.15'
const IPHONE_UA =
  'Mozilla/5.0 (iPhone; CPU iPhone OS 17_5 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1'
const LINUX_UA =
  'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36'

function manifest(): Record<string, unknown> {
  return {
    schema: 1,
    version: '0.2.0',
    released_at: '2026-11-05T10:00:00Z',
    min_supported: '0.1.0',
    notes: ['新增微信备份', '托盘菜单与退出修复'],
    history: [
      {
        version: '0.1.0',
        released_at: '2026-11-01T10:00:00Z',
        notes: ['首个版本：文件管理与文件夹自动备份'],
      },
      {
        version: '0.0.9',
        released_at: '2026-10-20T10:00:00Z',
        notes: ['内测版本'],
      },
    ],
    platforms: {
      'windows-x64': {
        file: 'EncryptedBackup-0.2.0-windows-x64.exe',
        size: 15_400_000,
        sha256: SHA_WINDOWS,
      },
      'macos-universal': {
        file: 'EncryptedBackup-0.2.0-macos.dmg',
        size: 38_500_000,
        sha256: SHA_MACOS,
      },
    },
  }
}

function jsonResponse(body: unknown) {
  return {
    ok: true,
    json: () => Promise.resolve(body),
  }
}

function stubFetch(
  implementation: (...args: Parameters<typeof fetch>) => Promise<unknown>,
) {
  const mock = vi.fn(implementation)
  vi.stubGlobal('fetch', mock)
  return mock
}

function stubNavigator({
  userAgent,
  platform,
}: {
  userAgent: string
  platform?: string
}) {
  Object.defineProperty(window.navigator, 'userAgent', {
    configurable: true,
    value: userAgent,
  })
  if (platform === undefined) {
    delete (
      window.navigator as unknown as { userAgentData?: unknown }
    ).userAgentData
    return
  }
  Object.defineProperty(window.navigator, 'userAgentData', {
    configurable: true,
    value: { platform },
  })
}

function stubClipboard() {
  const writeText = vi.fn().mockResolvedValue(undefined)
  Object.defineProperty(window.navigator, 'clipboard', {
    configurable: true,
    value: { writeText },
  })
  return writeText
}

function renderDownloadPage() {
  return render(
    <MemoryRouter initialEntries={['/download']}>
      <DownloadPage />
    </MemoryRouter>,
  )
}

const INVALID_CASES: Array<
  [string, (value: Record<string, unknown>) => void]
> = [
  ['版本不是 X.Y.Z', (value) => (value.version = '1.2')],
  [
    'SHA-256 不是 64 位小写十六进制',
    (value) => {
      const platforms = value.platforms as Record<
        string,
        Record<string, unknown>
      >
      platforms['windows-x64'].sha256 = 'ABC'
    },
  ],
  [
    '文件名包含路径',
    (value) => {
      const platforms = value.platforms as Record<
        string,
        Record<string, unknown>
      >
      platforms['windows-x64'].file = '../evil.exe'
    },
  ],
  ['字段缺失', (value) => delete value.notes],
]

afterEach(() => {
  vi.unstubAllGlobals()
  delete (window.navigator as unknown as { userAgentData?: unknown })
    .userAgentData
  delete (window.navigator as unknown as { clipboard?: unknown }).clipboard
  window.history.replaceState({}, '', '/')
})

describe('公开下载页', () => {
  it('Windows 访问者看到 Windows 主按钮、macOS 次要入口与放行说明', async () => {
    stubNavigator({ userAgent: WINDOWS_UA })
    const fetchMock = stubFetch(() => Promise.resolve(jsonResponse(manifest())))
    renderDownloadPage()

    const windows = await screen.findByTestId('download-windows')
    expect(windows).toHaveTextContent('下载 Windows 版')
    expect(windows).toHaveAttribute(
      'href',
      '/downloads/0.2.0/EncryptedBackup-0.2.0-windows-x64.exe',
    )
    expect(screen.queryByTestId('download-macos')).not.toBeInTheDocument()
    expect(screen.getByText('也有 macOS 版')).toBeInTheDocument()
    expect(screen.getByTestId('platform-windows')).toHaveAttribute(
      'data-current',
      'true',
    )
    expect(screen.getByTestId('platform-macos')).toHaveAttribute(
      'data-current',
      'false',
    )
    expect(screen.getByText(/版本 0\.2\.0/)).toBeInTheDocument()
    expect(screen.getAllByText('2026年11月5日').length).toBeGreaterThan(0)
    expect(screen.getAllByText('15.4 MB').length).toBeGreaterThan(0)
    const hash = screen.getByTitle(SHA_WINDOWS)
    expect(hash).toHaveClass('truncate')
    expect(hash).toHaveTextContent(SHA_WINDOWS)

    const firstOpen = screen.getByTestId('download-first-open')
    expect(firstOpen).toHaveTextContent('Windows 已保护你的电脑')
    expect(firstOpen).toHaveTextContent('更多信息')
    expect(firstOpen).toHaveTextContent('仍要运行')

    expect(fetchMock).toHaveBeenCalledWith('/downloads/latest.json', {
      cache: 'no-cache',
      credentials: 'omit',
    })
  })

  it('macOS 访问者看到 macOS 主按钮、Windows 次要入口与 macOS 截图', async () => {
    stubNavigator({ userAgent: MAC_UA })
    stubFetch(() => Promise.resolve(jsonResponse(manifest())))
    renderDownloadPage()

    const macos = await screen.findByTestId('download-macos')
    expect(macos).toHaveTextContent('下载 macOS 版')
    expect(macos).toHaveAttribute(
      'href',
      '/downloads/0.2.0/EncryptedBackup-0.2.0-macos.dmg',
    )
    expect(screen.queryByTestId('download-windows')).not.toBeInTheDocument()
    expect(screen.getByText('也有 Windows 版')).toBeInTheDocument()
    expect(screen.getByTestId('platform-macos')).toHaveAttribute(
      'data-current',
      'true',
    )
    expect(
      screen.getByRole('img', { name: '加密备份客户端界面' }),
    ).toHaveAttribute('src', expect.stringContaining('files-mac'))
  })

  it('iPhone 访问者看到电脑端提示与复制链接，不显示主下载按钮', async () => {
    stubNavigator({ userAgent: IPHONE_UA })
    stubFetch(() => Promise.resolve(jsonResponse(manifest())))
    const user = userEvent.setup()
    const writeText = stubClipboard()
    renderDownloadPage()

    expect(
      await screen.findByText(
        /客户端适用于 Windows 与 macOS 电脑，请在电脑上打开这个页面。iPhone\s*版即将推出。/,
      ),
    ).toBeVisible()
    expect(screen.queryByTestId('download-windows')).not.toBeInTheDocument()
    expect(screen.queryByTestId('download-macos')).not.toBeInTheDocument()
    expect(screen.getByTestId('platform-windows')).toHaveAttribute(
      'data-current',
      'false',
    )
    expect(screen.getByTestId('platform-macos')).toHaveAttribute(
      'data-current',
      'false',
    )
    expect(screen.getByTestId('platform-iphone')).toHaveTextContent('即将推出')

    await user.click(screen.getByTestId('copy-page-link'))
    expect(writeText).toHaveBeenCalledWith(window.location.href)
    expect(screen.getByTestId('copy-page-link')).toHaveTextContent('已复制')
  })

  it('Linux / 未知系统并列显示两个平台按钮', async () => {
    stubNavigator({ userAgent: LINUX_UA })
    stubFetch(() => Promise.resolve(jsonResponse(manifest())))
    renderDownloadPage()

    expect(await screen.findByTestId('download-windows')).toHaveTextContent(
      '下载 Windows 版',
    )
    expect(screen.getByTestId('download-macos')).toHaveTextContent(
      '下载 macOS 版',
    )
    expect(screen.queryByText('也有 macOS 版')).not.toBeInTheDocument()
    expect(screen.queryByText('也有 Windows 版')).not.toBeInTheDocument()
    expect(screen.getByTestId('platform-windows')).toHaveAttribute(
      'data-current',
      'false',
    )
    expect(screen.getByTestId('platform-macos')).toHaveAttribute(
      'data-current',
      'false',
    )
  })

  it('加载中只显示骨架，不显示任何版本号', async () => {
    stubNavigator({ userAgent: WINDOWS_UA })
    let resolveFetch:
      | ((value: Awaited<ReturnType<typeof jsonResponse>>) => void)
      | undefined
    stubFetch(
      () =>
        new Promise((resolve) => {
          resolveFetch = resolve
        }),
    )
    renderDownloadPage()

    expect(screen.getByTestId('download-loading')).toBeInTheDocument()
    expect(screen.getByTestId('platform-windows-loading')).toBeInTheDocument()
    expect(screen.queryByText(/0\.2\.0/)).not.toBeInTheDocument()

    resolveFetch?.(jsonResponse(manifest()))
    expect(await screen.findByTestId('download-windows')).toBeInTheDocument()
  })

  it('清单不可用时提示并提供重试；重试成功后显示版本信息', async () => {
    stubNavigator({ userAgent: WINDOWS_UA })
    stubFetch(() => {
      if (vi.mocked(fetch).mock.calls.length === 1) {
        return Promise.reject(new Error('offline'))
      }
      return Promise.resolve(jsonResponse(manifest()))
    })
    const user = userEvent.setup()
    renderDownloadPage()

    expect(
      await screen.findByTestId('download-manifest-error'),
    ).toHaveTextContent('暂时无法获取最新版本')
    expect(screen.getByTestId('download-windows-disabled')).toBeDisabled()
    expect(screen.getByTestId('download-macos-disabled')).toBeDisabled()
    expect(screen.queryByText(/0\.2\.0/)).not.toBeInTheDocument()
    expect(screen.queryByTestId('changelog')).not.toBeInTheDocument()

    await user.click(screen.getByTestId('download-retry'))
    expect(await screen.findByTestId('download-windows')).toHaveTextContent(
      '下载 Windows 版',
    )
    expect(screen.getByText(/版本 0\.2\.0/)).toBeInTheDocument()
  })

  it.each(INVALID_CASES)('非法清单（%s）按不可用处理', async (_label, change) => {
    stubNavigator({ userAgent: WINDOWS_UA })
    const invalid = manifest()
    change(invalid)
    stubFetch(() => Promise.resolve(jsonResponse(invalid)))
    renderDownloadPage()

    expect(
      await screen.findByTestId('download-manifest-error'),
    ).toHaveTextContent('暂时无法获取最新版本')
    expect(screen.getByTestId('download-windows-disabled')).toBeDisabled()
    expect(screen.queryByText(/0\.2\.0/)).not.toBeInTheDocument()
  })

  it('复制 SHA-256 后显示成功提示', async () => {
    stubNavigator({ userAgent: WINDOWS_UA })
    stubFetch(() => Promise.resolve(jsonResponse(manifest())))
    const user = userEvent.setup()
    const writeText = stubClipboard()
    renderDownloadPage()

    const copy = await screen.findByTestId('copy-sha-windows')
    await user.click(copy)

    expect(writeText).toHaveBeenCalledWith(SHA_WINDOWS)
    expect(copy).toHaveTextContent('已复制')
  })

  it('当前版本 notes 与 history 合并为最近 3 个版本的更新日志', async () => {
    stubNavigator({ userAgent: WINDOWS_UA })
    stubFetch(() => Promise.resolve(jsonResponse(manifest())))
    renderDownloadPage()

    const changelog = await screen.findByTestId('changelog')
    const entries = within(changelog).getAllByRole('article')
    expect(entries).toHaveLength(3)
    expect(entries[0]).toHaveTextContent('0.2.0')
    expect(entries[0]).toHaveTextContent('新增微信备份')
    expect(entries[1]).toHaveTextContent('0.1.0')
    expect(entries[1]).toHaveTextContent('首个版本：文件管理与文件夹自动备份')
    expect(entries[2]).toHaveTextContent('0.0.9')
  })

  it('未登录直接访问 /download 不跳转登录，也不创建 crypto worker', async () => {
    stubNavigator({ userAgent: WINDOWS_UA })
    const fetchMock = stubFetch(() =>
      Promise.resolve(jsonResponse(manifest())),
    )
    const worker = vi.fn()
    vi.stubGlobal('Worker', worker)
    window.history.pushState({}, '', '/download')

    render(<App />)

    expect(
      await screen.findByRole('heading', {
        name: /在电脑上自动加密备份.*你的文件/,
      }),
    ).toBeVisible()
    expect(screen.queryByTestId('login-email')).not.toBeInTheDocument()
    expect(await screen.findByTestId('download-windows')).toBeInTheDocument()
    expect(worker).not.toHaveBeenCalled()
    expect(fetchMock).toHaveBeenCalledTimes(1)
  })
})
