import { type ReactNode } from 'react'
import {
  Link,
  NavLink,
  Navigate,
  Outlet,
  useLocation,
  useNavigate,
} from 'react-router-dom'
import {
  ArrowLeft,
  ClipboardList,
  LayoutDashboard,
  LogOut,
  Settings,
  ShieldCheck,
  Users,
} from 'lucide-react'

import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import { NotFoundPage } from '../pages/NotFoundPage'
import { Brand } from '../shell/Brand'
import { Button, cx, Spinner } from '../ui'

/** 非管理员或未登录/未解锁时按各自语义处理；管理员才能进入后台。 */
export function RequireAdmin({ children }: { children: ReactNode }) {
  const { user, loading, unlocked } = useAuth()
  const location = useLocation()
  const from = `${location.pathname}${location.search}`
  if (loading) {
    return (
      <main className="flex min-h-dvh items-center justify-center bg-canvas">
        <Spinner size={28} label="加载中" />
      </main>
    )
  }
  if (!user) {
    return <Navigate to="/login" state={{ from }} replace />
  }
  if (!unlocked) {
    return <Navigate to="/unlock" state={{ from }} replace />
  }
  if (!user.isAdmin || user.status !== 'active') {
    return <NotFoundPage />
  }
  return <>{children}</>
}

const NAV = [
  { to: '/admin', label: '概览', icon: LayoutDashboard, end: true },
  { to: '/admin/users', label: '用户', icon: Users },
  { to: '/admin/settings', label: '设置', icon: Settings },
  { to: '/admin/audit', label: '审计日志', icon: ClipboardList },
]

/** 管理后台外壳：侧边栏 + 顶部说明（D6）。 */
export function AdminLayout() {
  const { user, logout } = useAuth()
  const navigate = useNavigate()

  return (
    <div className="min-h-dvh bg-white">
      <aside className="fixed inset-y-0 left-0 z-30 hidden w-60 flex-col border-r border-line bg-white px-3 pb-4 pt-5 md:flex">
        <div className="px-2">
          <Brand to="/admin" />
          <p className="mt-1 text-xs font-medium text-brand-600">管理后台</p>
        </div>
        <nav aria-label="管理后台导航" className="mt-6 flex flex-col gap-0.5">
          {NAV.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.end}
              className={({ isActive }) =>
                cx(
                  'flex items-center gap-3 rounded-control px-3 py-2 text-sm font-medium transition-colors motion-fast focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600',
                  isActive
                    ? 'bg-brand-50 text-brand-600'
                    : 'text-ink-600 hover:bg-canvas hover:text-ink-900',
                )
              }
            >
              <item.icon size={18} aria-hidden="true" />
              {item.label}
            </NavLink>
          ))}
        </nav>
        <div className="mt-auto space-y-1 border-t border-line px-1 pt-4">
          <Link
            to="/files"
            className="flex items-center gap-3 rounded-control px-3 py-2 text-sm font-medium text-ink-600 transition-colors motion-fast hover:bg-canvas hover:text-ink-900"
          >
            <ArrowLeft size={18} aria-hidden="true" />
            返回我的文件
          </Link>
          <button
            type="button"
            onClick={() => {
              void logout().finally(() => {
                void navigate('/login', { replace: true })
              })
            }}
            className="flex w-full items-center gap-3 rounded-control px-3 py-2 text-sm font-medium text-ink-600 transition-colors motion-fast hover:bg-canvas hover:text-ink-900"
          >
            <LogOut size={18} aria-hidden="true" />
            退出登录
          </button>
          <p className="truncate px-3 pt-2 text-xs text-ink-500">{user?.email}</p>
        </div>
      </aside>

      <div className="min-h-dvh md:pl-60">
        <header className="sticky top-0 z-20 flex min-h-16 items-center gap-3 border-b border-line bg-white px-4 py-3 sm:px-6 md:px-8">
          <div className="flex min-w-0 flex-1 items-start gap-2 text-xs leading-5 text-ink-600">
            <ShieldCheck
              size={16}
              className="mt-0.5 shrink-0 text-success-600"
              aria-hidden="true"
            />
            <p>
              管理员无法查看用户的文件名或内容。这里只展示服务端本来就有的账号、用量与健康信息。
            </p>
          </div>
        </header>
        <main className="bg-white px-4 py-5 sm:px-6 md:px-8 md:py-6">
          <div className="mx-auto w-full max-w-6xl">
            <Outlet />
          </div>
        </main>
      </div>
    </div>
  )
}

/** 会话超过 12 小时时，引导用户重新登录。 */
export function AdminReauthNotice() {
  const { logout } = useAuth()
  const navigate = useNavigate()
  return (
    <section
      data-testid="admin-reauth"
      className="mx-auto max-w-lg rounded-card border border-line bg-white p-8 text-center shadow-pop"
    >
      <ShieldCheck
        size={28}
        className="mx-auto mb-4 text-brand-600"
        aria-hidden="true"
      />
      <h1 className="text-xl font-semibold text-ink-900">需要重新登录</h1>
      <p className="mt-2 text-sm leading-6 text-ink-600">
        管理操作要求会话在最近 12 小时内通过密码登录创建。请重新登录后再继续。
      </p>
      <Button
        variant="primary"
        className="mt-6"
        data-testid="admin-reauth-login"
        onClick={() => {
          void logout().finally(() => {
            void navigate('/login', { replace: true })
          })
        }}
      >
        重新登录
      </Button>
    </section>
  )
}

export function AdminErrorBanner({ error }: { error: unknown }) {
  if (!error) {
    return null
  }
  return (
    <p
      role="alert"
      className="rounded-control border border-danger-600/30 bg-danger-50 px-3 py-2 text-sm text-danger-700"
    >
      {errorMessage(error)}
    </p>
  )
}
