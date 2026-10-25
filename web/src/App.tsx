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
import { PageShell } from './components/PageShell'
import { ChangePasswordPage } from './pages/ChangePasswordPage'
import { FilesPage } from './pages/FilesPage'
import { HomePage } from './pages/HomePage'
import { LoginPage } from './pages/LoginPage'
import { RegisterPage } from './pages/RegisterPage'
import { ResetPage } from './pages/ResetPage'
import { RotateRecoveryPage } from './pages/RotateRecoveryPage'
import { SecurityPage } from './pages/SecurityPage'
import { SessionsPage } from './pages/SessionsPage'
import { UnlockPage } from './pages/UnlockPage'
import { VerifyEmailPage } from './pages/VerifyEmailPage'

function RequireSession({ children }: { children: ReactNode }) {
  const { user, loading, unlocked } = useAuth()
  const location = useLocation()
  const from = `${location.pathname}${location.search}`
  if (loading) {
    return (
      <PageShell title="加载中" subtitle="正在检查会话…">
        <p role="status" className="text-sm text-slate-600">
          加载中…
        </p>
      </PageShell>
    )
  }
  if (!user) {
    return <Navigate to="/login" state={{ from }} replace />
  }
  if (!unlocked) {
    return <Navigate to="/unlock" state={{ from }} replace />
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
      <Route
        path="/"
        element={
          <RequireSession>
            <HomePage />
          </RequireSession>
        }
      />
      <Route
        path="/files"
        element={
          <RequireSession>
            <FilesPage />
          </RequireSession>
        }
      />
      <Route
        path="/sessions"
        element={
          <RequireSession>
            <SessionsPage />
          </RequireSession>
        }
      />
      <Route
        path="/change-password"
        element={
          <RequireSession>
            <ChangePasswordPage />
          </RequireSession>
        }
      />
      <Route
        path="/rotate-recovery-key"
        element={
          <RequireSession>
            <RotateRecoveryPage />
          </RequireSession>
        }
      />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  )
}

function App() {
  return (
    <BrowserRouter>
      <AuthProvider>
        <AppRoutes />
      </AuthProvider>
    </BrowserRouter>
  )
}

export default App
