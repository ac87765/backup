import { useState } from 'react'
import { Navigate, useLocation, useNavigate } from 'react-router-dom'

import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import { type AuthRedirectState, safeRedirect } from '../auth/navigation'
import { AuthLayout } from '../components/AuthLayout'
import { Button, FormError, Spinner, TextField } from '../ui'

export function UnlockPage() {
  const { api, user, loading, unlocked, markUnlocked, logout } = useAuth()
  const navigate = useNavigate()
  const location = useLocation()
  const destination = safeRedirect(
    (location.state as AuthRedirectState | null)?.from,
  )
  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  if (loading) {
    return (
      <AuthLayout title="解锁" subtitle="正在检查会话…">
        <div className="flex justify-center py-6">
          <Spinner size={24} label="加载中" />
        </div>
      </AuthLayout>
    )
  }
  if (!user) {
    return <Navigate to="/login" state={{ from: destination }} replace />
  }
  if (unlocked) {
    return <Navigate to={destination} replace />
  }

  const submit = async () => {
    setError(null)
    if (!api) {
      setError('当前环境不支持 Worker')
      return
    }
    setBusy(true)
    try {
      await api.unlock(password)
      setPassword('')
      markUnlocked()
      void navigate(destination, { replace: true })
    } catch (caught) {
      setError(errorMessage(caught))
    } finally {
      setBusy(false)
    }
  }

  return (
    <AuthLayout
      title="解锁"
      subtitle={`已登录为 ${user.email}。主密钥只存在于本页 Worker 内存，刷新后需要重新解锁。`}
      footer={
        <button
          type="button"
          onClick={() => {
            void logout()
              .catch(() => undefined)
              .finally(() => {
                void navigate('/login')
              })
          }}
          className="font-medium text-ink-600 hover:text-ink-900 hover:underline"
        >
          退出登录
        </button>
      }
    >
      <form
        className="space-y-4"
        onSubmit={(event) => {
          event.preventDefault()
          void submit()
        }}
      >
        <FormError message={error} />
        <TextField
          label="密码"
          data-testid="unlock-password"
          type="password"
          required
          autoComplete="current-password"
          value={password}
          onChange={(event) => setPassword(event.target.value)}
        />
        <Button
          type="submit"
          variant="primary"
          block
          data-testid="unlock-submit"
          disabled={busy}
        >
          {busy ? '解锁中…' : '解锁'}
        </Button>
      </form>
    </AuthLayout>
  )
}
