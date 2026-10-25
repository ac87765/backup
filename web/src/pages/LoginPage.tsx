import { useState } from 'react'
import { Link, Navigate, useLocation, useNavigate } from 'react-router-dom'

import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import { type AuthRedirectState, safeRedirect } from '../auth/navigation'
import { AuthLayout } from '../components/AuthLayout'
import { Button, FormError, TextField } from '../ui'

export function LoginPage() {
  const { api, user, unlocked, setSession } = useAuth()
  const navigate = useNavigate()
  const location = useLocation()
  const destination = safeRedirect(
    (location.state as AuthRedirectState | null)?.from,
  )
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  if (user && unlocked) {
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
      const current = await api.login(email.trim(), password)
      setSession(current, true)
      setPassword('')
      void navigate(destination, { replace: true })
    } catch (caught) {
      setError(errorMessage(caught))
    } finally {
      setBusy(false)
    }
  }

  return (
    <AuthLayout
      title="端对端加密备份"
      subtitle="使用邮箱和密码登录。主密钥只在本机 Worker 内解包，不会发送到服务端。"
      footer={
        <div className="flex flex-wrap items-center justify-center gap-x-4 gap-y-1.5">
          <Link to="/register" className="font-medium text-brand-600 hover:underline">
            注册
          </Link>
          <Link to="/reset" className="font-medium text-brand-600 hover:underline">
            忘记密码 / 恢复密钥重置
          </Link>
          <Link to="/security" className="text-ink-600 hover:text-ink-900 hover:underline">
            了解端对端加密与服务端可见信息
          </Link>
        </div>
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
          label="邮箱"
          data-testid="login-email"
          type="email"
          required
          autoComplete="email"
          value={email}
          onChange={(event) => setEmail(event.target.value)}
        />
        <TextField
          label="密码"
          data-testid="login-password"
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
          data-testid="login-submit"
          disabled={busy}
        >
          {busy ? '登录中…' : '登录'}
        </Button>
      </form>
    </AuthLayout>
  )
}
