import { useCallback, useState, type ReactNode } from 'react'
import {
  BrowserRouter,
  Navigate,
  Route,
  Routes,
  useLocation,
  useNavigate,
} from 'react-router-dom'

import { AuthProvider } from './auth/AuthProvider'
import { useAuth } from './auth/context'
import { AdminAuditPage } from './admin/AdminAuditPage'
import { AdminLayout, RequireAdmin } from './admin/AdminLayout'
import { AdminOverviewPage } from './admin/AdminOverviewPage'
import { AdminSettingsPage } from './admin/AdminSettingsPage'
import { AdminUsersPage } from './admin/AdminUsersPage'
import { DownloadPage } from './download/DownloadPage'
import { FilesPage } from './pages/FilesPage'
import { LoginPage } from './pages/LoginPage'
import { PendingPage } from './pages/PendingPage'
import { RegisterPage } from './pages/RegisterPage'
import { ResetPage } from './pages/ResetPage'
import { SecurityPage } from './pages/SecurityPage'
import { SettingsPage } from './pages/SettingsPage'
import { TrashPage } from './pages/TrashPage'
import { UnlockPage } from './pages/UnlockPage'
import { VerifyEmailPage } from './pages/VerifyEmailPage'
import { UsageProvider } from './shell/UsageProvider'
import { Spinner, ToastProvider } from './ui'

function RequireSession({ children }: { children: ReactNode }) {
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
  if (user.status === 'pending') {
    // pending 用户的全部登录态路由都显示等待审核页（add-admin-console D6）。
    return <PendingPage />
  }
  return <>{children}</>
}

function AppRoutes() {
  const navigate = useNavigate()
  const [emailToken, setEmailToken] = useState<string | null>(() => {
    const hash = window.location.hash
    return hash.startsWith('#token=')
      ? decodeURIComponent(hash.slice('#token='.length))
      : null
  })

  const clearHash = useCallback(() => {
    window.history.replaceState(
      null,
      '',
      `${window.location.pathname}${window.location.search}`,
    )
  }, [])

  const finishEmailVerification = useCallback(() => {
    // 成功或失败后立刻清除 URL 片段，避免令牌留在地址栏/历史记录。
    clearHash()
  }, [clearHash])

  if (emailToken) {
    return (
      <VerifyEmailPage
        token={emailToken}
        onFinished={finishEmailVerification}
        onDone={() => {
          setEmailToken(null)
          void navigate('/login', { replace: true })
        }}
      />
    )
  }

  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route path="/register" element={<RegisterPage />} />
      <Route
        path="/verify-email"
        element={
          <VerifyEmailPage
            token={null}
            onFinished={finishEmailVerification}
            onDone={() => {
              void navigate('/login', { replace: true })
            }}
          />
        }
      />
      <Route path="/reset" element={<ResetPage />} />
      <Route path="/security" element={<SecurityPage />} />
      <Route path="/unlock" element={<UnlockPage />} />

      <Route path="/" element={<Navigate to="/files" replace />} />
      <Route
        path="/files"
        element={
          <RequireSession>
            <FilesPage />
          </RequireSession>
        }
      />
      <Route
        path="/trash"
        element={
          <RequireSession>
            <TrashPage />
          </RequireSession>
        }
      />
      <Route
        path="/settings"
        element={
          <RequireSession>
            <SettingsPage />
          </RequireSession>
        }
      />

      <Route
        path="/admin"
        element={
          <RequireAdmin>
            <AdminLayout />
          </RequireAdmin>
        }
      >
        <Route index element={<AdminOverviewPage />} />
        <Route path="users" element={<AdminUsersPage />} />
        <Route path="settings" element={<AdminSettingsPage />} />
        <Route path="audit" element={<AdminAuditPage />} />
      </Route>

      {/* 旧地址兼容：重定向到账号设置对应分区。 */}
      <Route
        path="/change-password"
        element={
          <RequireSession>
            <Navigate to="/settings#password" replace />
          </RequireSession>
        }
      />
      <Route
        path="/rotate-recovery-key"
        element={
          <RequireSession>
            <Navigate to="/settings#recovery" replace />
          </RequireSession>
        }
      />
      <Route
        path="/sessions"
        element={
          <RequireSession>
            <Navigate to="/settings#sessions" replace />
          </RequireSession>
        }
      />

      <Route path="*" element={<Navigate to="/files" replace />} />
    </Routes>
  )
}

function App() {
  return (
    <BrowserRouter>
      <Routes>
        {/*
         * 公开下载页不挂 AuthProvider：不恢复会话、不创建 crypto worker，
         * 也不会因为未登录被重定向到登录页。已登录时同样可以直接访问。
         */}
        <Route path="/download" element={<DownloadPage />} />
        <Route
          path="*"
          element={
            <AuthProvider>
              <UsageProvider>
                <ToastProvider>
                  <AppRoutes />
                </ToastProvider>
              </UsageProvider>
            </AuthProvider>
          }
        />
      </Routes>
    </BrowserRouter>
  )
}

export default App
