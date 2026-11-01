import { Link, useLocation, useNavigate } from 'react-router-dom'
import { ChevronRight, KeyRound, Monitor, ShieldCheck } from 'lucide-react'

import { AppShell } from '../shell/AppShell'
import { PageHeader } from '../shell/PageHeader'
import { ChangePasswordSection } from '../settings/ChangePasswordSection'
import { RotateRecoverySection } from '../settings/RotateRecoverySection'
import { SessionsSection } from '../settings/SessionsSection'
import { cx } from '../ui'

type SectionId = 'password' | 'recovery' | 'sessions'

const SECTIONS: Array<{
  id: SectionId
  label: string
  icon: typeof KeyRound
}> = [
  { id: 'password', label: '修改密码', icon: KeyRound },
  { id: 'recovery', label: '恢复密钥', icon: ShieldCheck },
  { id: 'sessions', label: '登录设备', icon: Monitor },
]

const VALID_SECTIONS = new Set<string>(SECTIONS.map((section) => section.id))

function sectionFromHash(hash: string): SectionId {
  const value = hash.replace(/^#/, '')
  return VALID_SECTIONS.has(value) ? (value as SectionId) : 'password'
}

export function SettingsPage() {
  const location = useLocation()
  const navigate = useNavigate()
  const active = sectionFromHash(location.hash)

  return (
    <AppShell title="账号设置">
      <div
        data-testid="settings-page"
        className="mx-auto w-full max-w-[640px] space-y-6"
      >
        <PageHeader
          title="账号设置"
          subtitle="管理密码、恢复密钥与登录设备；这些操作只影响账号，不会重新加密已有文件。"
        />

        <div
          role="tablist"
          aria-label="账号设置分区"
          className="flex flex-wrap gap-1 rounded-card border border-line bg-white p-1"
        >
          {SECTIONS.map((section) => {
            const Icon = section.icon
            const selected = section.id === active
            return (
              <button
                key={section.id}
                type="button"
                role="tab"
                id={`settings-tab-${section.id}`}
                aria-selected={selected}
                aria-controls={`settings-panel-${section.id}`}
                onClick={() => void navigate(`/settings#${section.id}`)}
                className={cx(
                  'motion-fast inline-flex items-center gap-2 rounded-control px-3 py-2 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600',
                  selected
                    ? 'bg-brand-50 text-brand-600'
                    : 'text-ink-600 hover:bg-canvas hover:text-ink-900',
                )}
              >
                <Icon size={16} aria-hidden="true" />
                {section.label}
              </button>
            )
          })}
        </div>

        <section
          id={`settings-panel-${active}`}
          role="tabpanel"
          aria-labelledby={`settings-tab-${active}`}
          className="rounded-card border border-line bg-white p-6"
        >
          {active === 'password' && <ChangePasswordSection />}
          {active === 'recovery' && <RotateRecoverySection />}
          {active === 'sessions' && <SessionsSection />}
        </section>

        <Link
          to="/download"
          data-testid="settings-download-client"
          className="flex items-center gap-3 rounded-card border border-line bg-white p-4 transition-colors motion-fast hover:border-line-strong hover:bg-canvas focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600"
        >
          <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-control bg-brand-50 text-brand-600">
            <Monitor size={18} aria-hidden="true" />
          </span>
          <span className="min-w-0 flex-1">
            <span className="block text-sm font-medium text-ink-900">
              桌面客户端
            </span>
            <span className="block text-xs text-ink-500">
              下载 Windows 或 macOS 客户端，自动加密备份文件。
            </span>
          </span>
          <ChevronRight
            size={18}
            className="shrink-0 text-ink-400"
            aria-hidden="true"
          />
        </Link>
      </div>
    </AppShell>
  )
}
