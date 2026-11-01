import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react'
import { Link } from 'react-router-dom'
import {
  AlertTriangle,
  ArrowRight,
  Download,
  ExternalLink,
  Laptop,
  MessageCircle,
  Monitor,
  RefreshCw,
  ShieldCheck,
  Smartphone,
} from 'lucide-react'

import filesMac from '../assets/download/files-mac.png'
import filesWin from '../assets/download/files-win.png'
import { Brand } from '../shell/Brand'
import { Button, cx } from '../ui'
import { copyText } from './clipboard'
import {
  downloadUrl,
  fetchLatestManifest,
  formatInstallerSize,
  formatReleaseDate,
  mergeChangelog,
  type ChangelogEntry,
  type ReleaseManifest,
} from './manifest'
import { detectSystem, type VisitorSystem } from './system'
import './download.css'

type LoadState =
  | { status: 'loading' }
  | { status: 'error' }
  | { status: 'ready'; manifest: ReleaseManifest }

type CopyTarget = 'windows' | 'macos' | 'page'

interface CopyFeedback {
  target: CopyTarget
  ok: boolean
}

const FOCUS_RING =
  'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600 focus-visible:ring-offset-2'

const HERO_PRIMARY = cx(
  'inline-flex h-[52px] items-center justify-center gap-2.5 rounded-xl bg-brand-600 px-[26px] text-base font-medium text-white shadow-[0_6px_20px_rgba(0,97,254,0.25)] transition-colors motion-fast hover:bg-brand-700 active:bg-brand-700',
  FOCUS_RING,
)


const PLATFORM_ACTION = cx(
  'inline-flex h-10 w-full items-center justify-center gap-2 rounded-control px-4 text-sm font-medium transition-colors motion-fast',
  FOCUS_RING,
)

function platformPrimary() {
  return cx(PLATFORM_ACTION, 'bg-brand-600 text-white hover:bg-brand-700')
}

function platformSecondary() {
  return cx(
    PLATFORM_ACTION,
    'border border-line-strong bg-white text-ink-900 hover:border-line-strong hover:bg-canvas',
  )
}

function copyLabel(
  feedback: CopyFeedback | null,
  target: CopyTarget,
): string {
  if (feedback?.target !== target) {
    return '复制'
  }
  return feedback.ok ? '已复制' : '复制失败'
}

function HeroShell({ children }: { children: ReactNode }) {
  return <main className="flex-1">{children}</main>
}

function Section({
  id,
  children,
  alt = false,
  testId,
}: {
  id?: string
  children: ReactNode
  alt?: boolean
  testId?: string
}) {
  return (
    <section
      id={id}
      data-testid={testId}
      className={cx(
        'scroll-mt-20 px-4 py-9 sm:px-6 md:px-12 md:py-14',
        alt && 'border-y border-line bg-canvas',
      )}
    >
      {children}
    </section>
  )
}

function SectionHeading({
  title,
  subtitle,
}: {
  title: string
  subtitle: string
}) {
  return (
    <>
      <h2 className="text-2xl font-semibold tracking-tight text-ink-900">
        {title}
      </h2>
      <p className="mb-7 mt-1.5 text-sm text-ink-500">{subtitle}</p>
    </>
  )
}

function PlatformCard({
  os,
  state,
  current,
  feedback,
  onCopy,
}: {
  os: 'windows' | 'macos'
  state: LoadState
  current: boolean
  feedback: CopyFeedback | null
  onCopy: (target: 'windows' | 'macos', value: string) => void
}) {
  const manifest = state.status === 'ready' ? state.manifest : null
  const artifact = manifest
    ? os === 'windows'
      ? manifest.platforms.windows
      : manifest.platforms.macos
    : null
  const isWindows = os === 'windows'
  const Icon = isWindows ? Monitor : Laptop
  const title = isWindows ? 'Windows' : 'macOS'
  const requirement = isWindows
    ? 'Windows 10 / 11，64 位'
    : 'macOS 12 及以上，Apple 芯片与 Intel'
  const actionLabel = isWindows ? '下载 .exe 安装包' : '下载 .dmg'
  const buttonLabel =
    state.status === 'loading' ? '正在获取版本…' : '暂时无法下载'

  return (
    <article
      data-testid={`platform-${os}`}
      data-current={current ? 'true' : 'false'}
      className={cx(
        'flex min-w-0 flex-col gap-3.5 rounded-card border bg-white p-5',
        current ? 'border-brand-600 ring-[3px] ring-brand-50' : 'border-line',
      )}
    >
      <header className="flex items-center gap-3">
        <span
          className={cx(
            'flex h-11 w-11 shrink-0 items-center justify-center rounded-xl',
            current
              ? 'bg-brand-50 text-brand-600'
              : 'bg-canvas-strong text-ink-600',
          )}
        >
          <Icon size={22} aria-hidden="true" />
        </span>
        <div className="min-w-0">
          <b className="block text-base font-semibold text-ink-900">{title}</b>
          <span className="block text-xs text-ink-500">{requirement}</span>
        </div>
      </header>

      {state.status === 'loading' && (
        <div
          data-testid={`platform-${os}-loading`}
          aria-hidden="true"
          className="space-y-2.5 py-1"
        >
          <div className="skeleton h-3 w-3/4 rounded" />
          <div className="skeleton h-3 w-1/2 rounded" />
          <div className="skeleton h-3 w-full rounded" />
        </div>
      )}

      {state.status === 'error' && <div className="flex-1" />}

      {manifest && artifact && (
        <dl className="grid grid-cols-[72px_minmax(0,1fr)] gap-x-3 gap-y-2 text-sm">
          <dt className="text-ink-500">版本</dt>
          <dd className="min-w-0">
            {manifest.version}（{formatReleaseDate(manifest.releasedAt)}）
          </dd>
          <dt className="text-ink-500">大小</dt>
          <dd className="min-w-0">{formatInstallerSize(artifact.size)}</dd>
          <dt className="text-ink-500">SHA-256</dt>
          <dd className="min-w-0">
            <div className="flex items-center gap-1.5">
              <code
                title={artifact.sha256}
                className="block min-w-0 flex-1 truncate font-mono text-xs text-ink-600"
              >
                {artifact.sha256}
              </code>
              <button
                type="button"
                data-testid={`copy-sha-${os}`}
                aria-label={`复制 ${title} 安装包的 SHA-256`}
                onClick={() => onCopy(os, artifact.sha256)}
                className={cx(
                  'shrink-0 rounded text-xs font-medium text-brand-700 hover:underline',
                  FOCUS_RING,
                )}
              >
                {copyLabel(feedback, os)}
              </button>
            </div>
          </dd>
        </dl>
      )}

      <div className="mt-auto">
        {manifest && artifact ? (
          <a
            data-testid={`download-${os}-installer`}
            href={downloadUrl(manifest, os)}
            download
            className={current ? platformPrimary() : platformSecondary()}
          >
            <Download size={16} aria-hidden="true" />
            {actionLabel}
          </a>
        ) : (
          <button
            type="button"
            disabled
            data-testid={`download-${os}-disabled`}
            className={cx(
              PLATFORM_ACTION,
              current
                ? 'bg-brand-600 text-white'
                : 'border border-line-strong bg-white text-ink-500',
              'opacity-45',
            )}
          >
            <Download size={16} aria-hidden="true" />
            {buttonLabel}
          </button>
        )}
      </div>
    </article>
  )
}

function PhoneCard() {
  return (
    <article
      data-testid="platform-iphone"
      className="flex min-w-0 flex-col gap-3.5 rounded-card border border-dashed border-line bg-canvas p-5"
    >
      <header className="flex items-center gap-3">
        <span className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-canvas-strong text-ink-600">
          <Smartphone size={22} aria-hidden="true" />
        </span>
        <div className="min-w-0">
          <b className="block text-base font-semibold text-ink-900">iPhone</b>
          <span className="block text-xs text-ink-500">即将推出</span>
        </div>
      </header>
      <p className="text-xs leading-6 text-ink-500">
        现在可以在手机浏览器中使用网页版查看和下载文件。
      </p>
    </article>
  )
}

function Changelog({ entries }: { entries: ChangelogEntry[] }) {
  return (
    <div
      data-testid="changelog"
      className="overflow-hidden rounded-card border border-line bg-white"
    >
      {entries.map((entry) => (
        <article
          key={`${entry.version}-${entry.releasedAt}`}
          className="grid gap-1.5 border-b border-line px-5 py-4 last:border-b-0 md:grid-cols-[160px_minmax(0,1fr)] md:gap-4"
        >
          <div>
            <b className="block text-sm font-semibold text-ink-900">
              {entry.version}
            </b>
            <span className="text-xs text-ink-500">
              {formatReleaseDate(entry.releasedAt)}
            </span>
          </div>
          {entry.notes.length > 0 ? (
            <ul className="list-disc space-y-1 pl-5 text-sm leading-7 text-ink-600">
              {entry.notes.map((note, index) => (
                <li key={`${entry.version}-${index}`}>{note}</li>
              ))}
            </ul>
          ) : (
            <p className="text-sm text-ink-500">此版本没有更新说明。</p>
          )}
        </article>
      ))}
    </div>
  )
}

function StepList({ steps }: { steps: ReactNode[] }) {
  return (
    <ol className="space-y-3">
      {steps.map((step, index) => (
        <li
          key={index}
          className="grid grid-cols-[26px_minmax(0,1fr)] gap-2.5 text-sm leading-7 text-ink-600"
        >
          <span className="flex h-[26px] w-[26px] items-center justify-center rounded-full bg-brand-50 text-xs font-semibold text-brand-700">
            {index + 1}
          </span>
          <span className="min-w-0">{step}</span>
        </li>
      ))}
    </ol>
  )
}

function CodeBlock({ children }: { children: ReactNode }) {
  return (
    <pre className="mt-2 overflow-x-auto rounded-control bg-canvas px-3 py-2 text-xs text-ink-900">
      <code className="font-mono">{children}</code>
    </pre>
  )
}

export function DownloadPage() {
  const system = useMemo<VisitorSystem>(() => detectSystem(), [])
  const [state, setState] = useState<LoadState>({ status: 'loading' })
  const [attempt, setAttempt] = useState(0)
  const [feedback, setFeedback] = useState<CopyFeedback | null>(null)
  const feedbackTimer = useRef<number | null>(null)

  useEffect(() => {
    let cancelled = false
    fetchLatestManifest()
      .then((manifest) => {
        if (!cancelled) {
          setState({ status: 'ready', manifest })
        }
      })
      .catch(() => {
        if (!cancelled) {
          setState({ status: 'error' })
        }
      })
    return () => {
      cancelled = true
    }
  }, [attempt])

  useEffect(() => {
    const previous = document.title
    document.title = '下载客户端 · 加密备份'
    return () => {
      document.title = previous
    }
  }, [])

  useEffect(() => {
    return () => {
      if (feedbackTimer.current !== null) {
        window.clearTimeout(feedbackTimer.current)
      }
    }
  }, [])

  const showFeedback = useCallback((target: CopyTarget, ok: boolean) => {
    setFeedback({ target, ok })
    if (feedbackTimer.current !== null) {
      window.clearTimeout(feedbackTimer.current)
    }
    feedbackTimer.current = window.setTimeout(() => {
      setFeedback(null)
      feedbackTimer.current = null
    }, 2_000)
  }, [])

  const copy = useCallback(
    async (target: CopyTarget, value: string) => {
      showFeedback(target, await copyText(value))
    },
    [showFeedback],
  )

  const copySha = useCallback(
    (target: 'windows' | 'macos', value: string) => {
      void copy(target, value)
    },
    [copy],
  )

  const copyPageLink = useCallback(() => {
    void copy('page', window.location.href)
  }, [copy])

  const retry = useCallback(() => {
    setState({ status: 'loading' })
    setAttempt((value) => value + 1)
  }, [])

  const manifest = state.status === 'ready' ? state.manifest : null
  const screenshot = system === 'macos' ? filesMac : filesWin
  const changelog = manifest ? mergeChangelog(manifest) : []

  return (
    <div
      data-testid="download-page"
      className="download-page min-h-dvh bg-canvas-strong"
    >
      <div className="mx-auto w-full max-w-[1280px] bg-white text-ink-900">
        <nav
          aria-label="顶部导航"
          className="flex h-16 items-center gap-3 border-b border-line px-4 md:gap-7 md:px-12"
        >
          <Brand to="/" size="sm" />
          <div className="hidden items-center gap-6 md:flex">
            <a
              href="#features"
              className={cx(
                'rounded text-sm text-ink-600 hover:text-ink-900',
                FOCUS_RING,
              )}
            >
              功能
            </a>
            <Link
              to="/security"
              className={cx(
                'rounded text-sm text-ink-600 hover:text-ink-900',
                FOCUS_RING,
              )}
            >
              安全
            </Link>
            <Link
              to="/download"
              aria-current="page"
              className={cx(
                'rounded text-sm font-medium text-ink-900',
                FOCUS_RING,
              )}
            >
              下载
            </Link>
          </div>
          <span className="flex-1" />
          <Link
            to="/login"
            className={cx(
              'hidden rounded text-sm text-ink-600 hover:text-ink-900 md:inline',
              FOCUS_RING,
            )}
          >
            登录网页版
          </Link>
          <Link
            to="/register"
            className={cx(
              'inline-flex h-8 items-center rounded-control bg-brand-600 px-3 text-sm font-medium text-white transition-colors motion-fast hover:bg-brand-700',
              FOCUS_RING,
            )}
          >
            注册
          </Link>
        </nav>

        <HeroShell>
          <section className="download-hero relative overflow-hidden px-4 py-10 sm:px-6 md:grid md:grid-cols-[minmax(0,1fr)_560px] md:items-center md:gap-12 md:px-12 md:py-[72px]">
            <div className="download-enter min-w-0">
              <h1 className="m-0 text-[28px] font-semibold leading-tight tracking-tight text-ink-900 md:text-[40px] md:leading-[1.2]">
                在电脑上自动加密备份
                <br />
                你的文件
              </h1>
              <p className="mt-3.5 max-w-[520px] text-sm leading-7 text-ink-600 md:text-base md:leading-[1.7]">
                文件在你的电脑上加密后才上传，服务器只保存密文。选好文件夹，之后新增和修改的文件都会自动备份，还能单独备份微信。
              </p>

              {state.status === 'loading' && (
                <div
                  data-testid="download-loading"
                  aria-busy="true"
                  aria-label="正在获取最新版本"
                  className="mt-7 space-y-3"
                >
                  <div className="skeleton h-[52px] w-48 rounded-xl" />
                  <div className="skeleton h-4 w-64 max-w-full rounded" />
                </div>
              )}

              {state.status === 'error' && (
                <div
                  data-testid="download-manifest-error"
                  role="alert"
                  className="mt-7 flex max-w-[520px] items-center gap-3 rounded-card bg-warning-50 px-4 py-3 text-sm text-warning-600"
                >
                  <AlertTriangle size={18} aria-hidden="true" />
                  <span className="min-w-0 flex-1">
                    暂时无法获取最新版本，请稍后再试。
                  </span>
                  <Button
                    size="sm"
                    variant="secondary"
                    data-testid="download-retry"
                    onClick={retry}
                  >
                    重试
                  </Button>
                </div>
              )}

              {system === 'mobile' && (
                <div className="mt-7 flex max-w-[520px] flex-col gap-3">
                  <div className="flex gap-2.5 rounded-control bg-brand-50 px-3.5 py-3 text-xs leading-6 text-brand-700">
                    <Monitor
                      size={16}
                      className="mt-0.5 shrink-0"
                      aria-hidden="true"
                    />
                    <span>
                      客户端适用于 Windows 与 macOS 电脑，请在电脑上打开这个页面。iPhone
                      版即将推出。
                    </span>
                  </div>
                  <Button
                    size="md"
                    variant="secondary"
                    block
                    data-testid="copy-page-link"
                    icon={<ExternalLink size={16} />}
                    onClick={copyPageLink}
                  >
                    {feedback?.target === 'page'
                      ? feedback.ok
                        ? '已复制链接'
                        : '复制失败'
                      : '复制下载页面链接'}
                  </Button>
                </div>
              )}

              {(manifest && system !== 'mobile') && (
                <>
                  <div className="mt-7 flex flex-wrap items-center gap-4">
                    {(system === 'windows' || system === 'other') && (
                      <a
                        data-testid="download-windows"
                        href={downloadUrl(manifest, 'windows')}
                        download
                        className={HERO_PRIMARY}
                      >
                        <Download size={20} aria-hidden="true" />
                        下载 Windows 版
                      </a>
                    )}
                    {(system === 'macos' || system === 'other') && (
                      <a
                        data-testid="download-macos"
                        href={downloadUrl(manifest, 'macos')}
                        download
                        className={HERO_PRIMARY}
                      >
                        <Download size={20} aria-hidden="true" />
                        下载 macOS 版
                      </a>
                    )}
                    {system === 'windows' && (
                      <a
                        href="#platform-macos"
                        className={cx(
                          'inline-flex items-center gap-1 rounded text-sm text-brand-700 hover:underline',
                          FOCUS_RING,
                        )}
                      >
                        也有 macOS 版
                        <ArrowRight size={14} aria-hidden="true" />
                      </a>
                    )}
                    {system === 'macos' && (
                      <a
                        href="#platform-windows"
                        className={cx(
                          'inline-flex items-center gap-1 rounded text-sm text-brand-700 hover:underline',
                          FOCUS_RING,
                        )}
                      >
                        也有 Windows 版
                        <ArrowRight size={14} aria-hidden="true" />
                      </a>
                    )}
                  </div>

                  {system !== 'other' ? (
                    <div className="mt-4 flex flex-wrap items-center gap-x-2.5 gap-y-1 text-sm text-ink-500">
                      <span>版本 {manifest.version}</span>
                      <span className="h-[3px] w-[3px] rounded-full bg-ink-400" />
                      <span>{formatReleaseDate(manifest.releasedAt)}</span>
                      <span className="h-[3px] w-[3px] rounded-full bg-ink-400" />
                      <span>
                        {system === 'macos'
                          ? `${formatInstallerSize(manifest.platforms.macos.size)} · macOS 12 及以上`
                          : `${formatInstallerSize(manifest.platforms.windows.size)} · Windows 10 / 11`}
                      </span>
                    </div>
                  ) : (
                    <div className="mt-4 flex flex-wrap items-center gap-x-2.5 gap-y-1 text-sm text-ink-500">
                      <span>版本 {manifest.version}</span>
                      <span className="h-[3px] w-[3px] rounded-full bg-ink-400" />
                      <span>{formatReleaseDate(manifest.releasedAt)}</span>
                      <span className="h-[3px] w-[3px] rounded-full bg-ink-400" />
                      <span>
                        Windows {formatInstallerSize(manifest.platforms.windows.size)}
                      </span>
                      <span className="h-[3px] w-[3px] rounded-full bg-ink-400" />
                      <span>
                        macOS {formatInstallerSize(manifest.platforms.macos.size)}
                      </span>
                    </div>
                  )}
                </>
              )}
            </div>

            <div className="download-shot mt-7 min-w-0 overflow-hidden rounded-[14px] shadow-[0_30px_70px_rgba(30,25,25,0.18),0_0_0_1px_rgba(0,0,0,0.06)] md:mt-0">
              <img
                src={screenshot}
                alt="加密备份客户端界面"
                className="block h-auto w-full"
              />
            </div>
          </section>

          <Section id="features" testId="download-features">
            <SectionHeading
              title="为什么用客户端"
              subtitle="网页版随时可用；客户端适合需要自动备份的电脑。"
            />
            <div className="grid gap-4 md:grid-cols-3">
              {[
                {
                  icon: <ShieldCheck size={20} aria-hidden="true" />,
                  title: '本机加密',
                  body: '密码和密钥只在你的电脑上，服务器和我们都看不到文件内容与文件名。',
                },
                {
                  icon: <RefreshCw size={20} aria-hidden="true" />,
                  title: '文件夹自动备份',
                  body: '选好文件夹后自动增量备份，修改过的文件保留旧版本，本地删除不会删掉云端。',
                },
                {
                  icon: <MessageCircle size={20} aria-hidden="true" />,
                  title: '微信备份',
                  body: '聊天文件、图片视频和聊天记录一起备份，换电脑后可以恢复。不读取聊天内容。',
                },
              ].map((feature) => (
                <article
                  key={feature.title}
                  className="rounded-card border border-line bg-white p-5"
                >
                  <span className="mb-3.5 flex h-10 w-10 items-center justify-center rounded-[10px] bg-brand-50 text-brand-600">
                    {feature.icon}
                  </span>
                  <b className="block text-sm font-semibold text-ink-900">
                    {feature.title}
                  </b>
                  <span className="mt-1.5 block text-sm leading-7 text-ink-600">
                    {feature.body}
                  </span>
                </article>
              ))}
            </div>
          </Section>

          <Section alt testId="download-platforms">
            <SectionHeading
              title="选择你的系统"
              subtitle="安装包由维护者签名发布，客户端会自动检查并安装更新。"
            />
            <div className="grid gap-4 md:grid-cols-[1fr_1fr_0.8fr]">
              <div id="platform-windows" className="scroll-mt-20">
                <PlatformCard
                  os="windows"
                  state={state}
                  current={system === 'windows'}
                  feedback={feedback}
                  onCopy={copySha}
                />
              </div>
              <div id="platform-macos" className="scroll-mt-20">
                <PlatformCard
                  os="macos"
                  state={state}
                  current={system === 'macos'}
                  feedback={feedback}
                  onCopy={copySha}
                />
              </div>
              <PhoneCard />
            </div>
          </Section>

          <Section testId="download-first-open">
            <SectionHeading
              title="第一次打开"
              subtitle="客户端暂未购买系统厂商的签名证书，第一次打开时系统会提示，按下面的步骤放行即可。"
            />
            <div className="grid gap-4 md:grid-cols-2">
              <div className="rounded-card border border-line bg-white p-5">
                <h3 className="mb-3.5 flex items-center gap-2 text-sm font-semibold text-ink-900">
                  <Monitor size={16} aria-hidden="true" />
                  Windows
                </h3>
                <StepList
                  steps={[
                    <>
                      双击下载的安装包。如果出现“Windows 已保护你的电脑”，点{' '}
                      <b className="font-medium text-ink-900">更多信息</b>，再点{' '}
                      <b className="font-medium text-ink-900">仍要运行</b>。
                    </>,
                    <>
                      按提示完成安装，不需要管理员权限。安全软件（如火绒）拦截时，选择{' '}
                      <b className="font-medium text-ink-900">允许</b>{' '}
                      或把安装包加入信任。
                    </>,
                    <>打开“加密备份”，用网页版的账号登录。</>,
                  ]}
                />
              </div>
              <div className="rounded-card border border-line bg-white p-5">
                <h3 className="mb-3.5 flex items-center gap-2 text-sm font-semibold text-ink-900">
                  <Laptop size={16} aria-hidden="true" />
                  macOS
                </h3>
                <StepList
                  steps={[
                    <>
                      打开下载的 .dmg，把“加密备份”拖到{' '}
                      <b className="font-medium text-ink-900">应用程序</b>。
                    </>,
                    <>
                      第一次打开时在“应用程序”中{' '}
                      <b className="font-medium text-ink-900">
                        右键点应用 → 打开
                      </b>
                      ，再点 <b className="font-medium text-ink-900">打开</b>
                      ；或在“系统设置 → 隐私与安全性”中允许。
                    </>,
                    <>备份文件夹需要读取权限时，按提示在系统设置中允许。</>,
                  ]}
                />
              </div>
            </div>
          </Section>

          <Section testId="download-verify">
            <div id="verify" className="scroll-mt-20">
              <SectionHeading
                title="如何校验安装包"
                subtitle="网页只展示服务器上的信息，安装包是否完整由你核对 SHA-256 确认；已安装客户端的自动更新会验证维护者签名。"
              />
              <div className="grid gap-4 md:grid-cols-2">
                <div className="rounded-card border border-line bg-white p-5">
                  <h3 className="mb-1 flex items-center gap-2 text-sm font-semibold text-ink-900">
                    <Monitor size={16} aria-hidden="true" />
                    Windows
                  </h3>
                  <p className="text-sm leading-7 text-ink-600">
                    在 PowerShell 中运行下面的命令，将输出与上方 SHA-256 对比：
                  </p>
                  <CodeBlock>Get-FileHash &lt;文件&gt; -Algorithm SHA256</CodeBlock>
                </div>
                <div className="rounded-card border border-line bg-white p-5">
                  <h3 className="mb-1 flex items-center gap-2 text-sm font-semibold text-ink-900">
                    <Laptop size={16} aria-hidden="true" />
                    macOS
                  </h3>
                  <p className="text-sm leading-7 text-ink-600">
                    在“终端”中运行下面的命令，将输出与上方 SHA-256 对比：
                  </p>
                  <CodeBlock>shasum -a 256 &lt;文件&gt;</CodeBlock>
                </div>
              </div>
              <div className="mt-4 rounded-card border border-line bg-white p-5 text-sm leading-7 text-ink-600">
                <h3 className="mb-1 flex items-center gap-2 text-sm font-semibold text-ink-900">
                  <ShieldCheck size={16} aria-hidden="true" />
                  公钥与签名
                </h3>
                <p>
                  发布清单与安装包由维护者用 Ed25519
                  私钥签名。发布公钥{' '}
                  <code className="rounded bg-canvas px-1 py-0.5 font-mono text-xs text-ink-900">
                    deploy/release-signing.pub
                  </code>{' '}
                  的内容可在 GitHub 仓库核对：
                </p>
                <a
                  href="https://github.com/ac87765/backup/blob/main/deploy/release-signing.pub"
                  target="_blank"
                  rel="noreferrer"
                  className={cx(
                    'mt-2 inline-flex items-center gap-1 rounded font-medium text-brand-700 hover:underline',
                    FOCUS_RING,
                  )}
                >
                  在 GitHub 查看发布公钥
                  <ExternalLink size={14} aria-hidden="true" />
                </a>
              </div>
            </div>
          </Section>

          {manifest && (
            <Section alt testId="download-changelog">
              <SectionHeading
                title="更新日志"
                subtitle="已安装的客户端会自动提示新版本。"
              />
              <Changelog entries={changelog} />
            </Section>
          )}
        </HeroShell>

        <footer className="flex flex-col items-start gap-2 border-t border-line px-4 py-6 text-xs text-ink-500 md:flex-row md:items-center md:gap-6 md:px-12">
          <span>© {new Date().getFullYear()} 加密备份</span>
          <Link
            to="/security"
            className={cx('rounded hover:text-ink-900', FOCUS_RING)}
          >
            安全与隐私
          </Link>
          <a
            href="https://github.com/ac87765/backup"
            target="_blank"
            rel="noreferrer"
            className={cx('rounded hover:text-ink-900', FOCUS_RING)}
          >
            源代码（GitHub）
          </a>
          <a
            href="#verify"
            className={cx('rounded hover:text-ink-900', FOCUS_RING)}
          >
            如何校验安装包
          </a>
        </footer>
      </div>
    </div>
  )
}
