import { useState, type ReactNode } from 'react'
import { NavLink, useNavigate } from 'react-router-dom'
import {
  CheckCircle2,
  ChevronDown,
  CircleAlert,
  FolderOpen,
  FolderPlus,
  LogOut,
  Menu as MenuIcon,
  Settings,
  ShieldCheck,
  Trash2,
  Upload,
  X,
} from 'lucide-react'

import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import { formatBytes } from '../files/format'
import {
  Button,
  cx,
  IconButton,
  Menu,
  ProgressBar,
  useToast,
  type MenuItem,
} from '../ui'
import { Brand } from './Brand'
import { useUsage } from './usage-context'

export interface AppShellProps {
  children: ReactNode
  /** 顶部栏左侧面包屑；外层自动带 data-testid="breadcrumb"。 */
  breadcrumb?: ReactNode
  /** 顶部栏右侧（筛选、视图切换等），位于账号菜单之前。 */
  toolbar?: ReactNode
  /** 当前页面能直接处理上传时提供；否则上传入口跳转到文件页。 */
  onUploadRequest?: () => void
  onNewFolderRequest?: () => void
}

interface SidebarBodyProps {
  onUploadRequest?: () => void
  onNewFolderRequest?: () => void
  onNavigate?: () => void
}

function SidebarBody({
  onUploadRequest,
  onNewFolderRequest,
  onNavigate,
}: SidebarBodyProps) {
  const navigate = useNavigate()
  const { usage } = useUsage()

  const runUpload = () => {
    onNavigate?.()
    if (onUploadRequest) {
      onUploadRequest()
    } else {
      void navigate('/files')
    }
  }

  const runNewFolder = () => {
    onNavigate?.()
    if (onNewFolderRequest) {
      onNewFolderRequest()
    } else {
      void navigate('/files')
    }
  }

  const uploadItems: MenuItem[] = [
    {
      label: '上传文件',
      testId: 'upload-file',
      ariaLabel: '上传文件',
      icon: <Upload size={16} />,
      onSelect: runUpload,
    },
    {
      label: '新建文件夹',
      testId: 'new-folder',
      ariaLabel: '新建文件夹',
      icon: <FolderPlus size={16} />,
      onSelect: runNewFolder,
    },
  ]

  const used = usage?.usedBytes ?? 0
  const quota = usage?.quotaBytes ?? 0
  const reserved = usage?.reservedBytes ?? 0
  const percent = quota > 0 ? (used / quota) * 100 : 0
  const nearQuota = percent >= 90

  return (
    <>
      <div className="mt-5 px-1">
        <Menu
          label="上传与新建"
          align="start"
          items={uploadItems}
          trigger={
            <Button
              variant="primary"
              block
              data-testid="upload-menu"
              icon={<Upload size={17} />}
              justify="between"
            >
              <span>上传</span>
              <ChevronDown size={16} aria-hidden="true" />
            </Button>
          }
        />
      </div>

      <nav aria-label="主导航" className="mt-5 flex flex-col gap-0.5">
        <NavLink
          to="/files"
          onClick={onNavigate}
          className={({ isActive }) =>
            cx(
              'flex items-center gap-3 rounded-control px-3 py-2 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600',
              isActive
                ? 'bg-brand-50 text-brand-600'
                : 'text-ink-600 hover:bg-canvas hover:text-ink-900',
            )
          }
        >
          <FolderOpen size={18} aria-hidden="true" />
          全部文件
        </NavLink>
        <NavLink
          to="/trash"
          data-testid="open-trash"
          onClick={onNavigate}
          className={({ isActive }) =>
            cx(
              'flex items-center gap-3 rounded-control px-3 py-2 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600',
              isActive
                ? 'bg-brand-50 text-brand-600'
                : 'text-ink-600 hover:bg-canvas hover:text-ink-900',
            )
          }
        >
          <Trash2 size={18} aria-hidden="true" />
          回收站
        </NavLink>
        <NavLink
          to="/security"
          onClick={onNavigate}
          className={({ isActive }) =>
            cx(
              'flex items-center gap-3 rounded-control px-3 py-2 text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600',
              isActive
                ? 'bg-brand-50 text-brand-600'
                : 'text-ink-600 hover:bg-canvas hover:text-ink-900',
            )
          }
        >
          <ShieldCheck size={18} aria-hidden="true" />
          安全与隐私
        </NavLink>
      </nav>

      <div className="mt-auto space-y-2.5 border-t border-line px-1 pt-4">
        <div
          data-testid="usage-summary"
          className={cx(
            'space-y-0.5 text-xs leading-5',
            nearQuota ? 'text-danger-600' : 'text-ink-600',
          )}
        >
          <p className={cx('font-medium', nearQuota ? 'text-danger-600' : 'text-ink-900')}>
            {usage
              ? `已用 ${formatBytes(used)} / 共 ${formatBytes(quota)}`
              : '用量加载中…'}
          </p>
          <p>{usage ? `预留 ${formatBytes(reserved)}` : '预留 —'}</p>
        </div>
        <ProgressBar
          value={percent}
          tone={nearQuota ? 'danger' : 'brand'}
          label="存储用量"
          heightClassName="h-2"
        />
      </div>
    </>
  )
}

function AccountMenu() {
  const { api, user, logout } = useAuth()
  const navigate = useNavigate()
  const { showToast } = useToast()
  const [busy, setBusy] = useState(false)
  const email = user?.email ?? ''
  const verified = user?.emailVerified ?? false
  const initial = email.trim().charAt(0).toUpperCase() || '?'

  const resend = async () => {
    if (!api) {
      return
    }
    try {
      await api.resendVerification(email)
      showToast('如果该邮箱尚未验证，我们已重新发送验证邮件。', 'info')
    } catch (caught) {
      showToast(errorMessage(caught), 'error')
    }
  }

  const signOut = () => {
    setBusy(true)
    void logout()
      .catch(() => undefined)
      .finally(() => {
        setBusy(false)
        void navigate('/login', { replace: true })
      })
  }

  const items: MenuItem[] = [
    {
      label: '账号设置',
      icon: <Settings size={16} />,
      onSelect: () => void navigate('/settings'),
    },
    {
      label: '安全与隐私',
      icon: <ShieldCheck size={16} />,
      onSelect: () => void navigate('/security'),
    },
    {
      label: '退出登录',
      icon: <LogOut size={16} />,
      tone: 'danger',
      separatorBefore: true,
      disabled: busy,
      onSelect: signOut,
    },
  ]

  return (
    <Menu
      label="账号菜单"
      items={items}
      header={
        <div className="space-y-1">
          <p className="truncate text-sm font-medium text-ink-900">{email}</p>
          <p
            data-testid="home-verified"
            className="flex items-center gap-1 text-xs text-ink-600"
          >
            {verified ? (
              <>
                <CheckCircle2
                  size={13}
                  className="text-success-600"
                  aria-hidden="true"
                />
                邮箱已验证
              </>
            ) : (
              <>
                <CircleAlert
                  size={13}
                  className="text-warning-600"
                  aria-hidden="true"
                />
                邮箱待验证
              </>
            )}
          </p>
          {!verified && (
            <button
              type="button"
              onClick={() => void resend()}
              className="rounded-control text-xs font-medium text-brand-600 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600"
            >
              重新发送验证邮件
            </button>
          )}
        </div>
      }
      trigger={
        <button
          type="button"
          data-testid="account-menu-trigger"
          aria-label={`账号菜单 ${email}`}
          className="flex h-9 items-center gap-2 rounded-full pl-1 pr-2 text-ink-600 transition-colors hover:bg-canvas hover:text-ink-900 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600"
        >
          <span
            aria-hidden="true"
            className="flex h-8 w-8 items-center justify-center rounded-full bg-brand-600 text-sm font-semibold text-white"
          >
            {initial}
          </span>
          <ChevronDown size={16} aria-hidden="true" />
          <span data-testid="home-email" className="sr-only">
            {email}
          </span>
        </button>
      }
    />
  )
}

function AppShellFrame({
  children,
  breadcrumb,
  toolbar,
  onUploadRequest,
  onNewFolderRequest,
}: AppShellProps) {
  const [drawerOpen, setDrawerOpen] = useState(false)

  return (
    <div className="min-h-dvh bg-canvas">
      <aside className="fixed inset-y-0 left-0 z-30 hidden w-60 flex-col border-r border-line bg-white px-3 pb-4 pt-5 md:flex">
        <div className="px-2">
          <Brand to="/files" />
        </div>
        <SidebarBody
          onUploadRequest={onUploadRequest}
          onNewFolderRequest={onNewFolderRequest}
        />
      </aside>

      <div className="md:pl-60">
        <header className="sticky top-0 z-20 flex h-16 items-center gap-2 border-b border-line bg-white px-3 sm:px-5 md:px-8">
          <IconButton
            label="打开导航菜单"
            className="md:hidden"
            onClick={() => setDrawerOpen(true)}
          >
            <MenuIcon size={20} />
          </IconButton>
          <div
            data-testid="breadcrumb"
            className="min-w-0 flex-1 overflow-hidden"
          >
            {breadcrumb}
          </div>
          <div className="flex shrink-0 items-center gap-1.5 sm:gap-2">
            {toolbar}
            <AccountMenu />
          </div>
        </header>

        <main className="px-4 py-5 sm:px-6 md:px-8 md:py-6">
          <div className="mx-auto w-full max-w-6xl">{children}</div>
        </main>
      </div>

      {drawerOpen && (
        <div className="fixed inset-0 z-40 md:hidden">
          <div
            aria-hidden="true"
            className="absolute inset-0 bg-ink-900/40"
            onClick={() => setDrawerOpen(false)}
          />
          <div
            role="dialog"
            aria-modal="true"
            aria-label="导航菜单"
            className="absolute inset-y-0 left-0 flex w-72 max-w-[85vw] flex-col bg-white px-3 pb-4 pt-4 shadow-pop"
          >
            <div className="flex items-center justify-between px-2">
              <Brand to="/files" />
              <IconButton
                label="关闭导航菜单"
                size="sm"
                onClick={() => setDrawerOpen(false)}
              >
                <X size={18} />
              </IconButton>
            </div>
            <SidebarBody
              onUploadRequest={onUploadRequest}
              onNewFolderRequest={onNewFolderRequest}
              onNavigate={() => setDrawerOpen(false)}
            />
          </div>
        </div>
      )}
    </div>
  )
}

/**
 * 登录后的统一外壳：左侧导航 + 顶部栏 + 主内容区。
 * 用量由 App 根部的 UsageProvider 提供，侧边栏与文件操作共享同一份状态。
 */
export function AppShell(props: AppShellProps) {
  return <AppShellFrame {...props} />
}
