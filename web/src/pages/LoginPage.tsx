import { useState } from 'react'
import {
  Link,
  Navigate,
  useLocation,
  useNavigate,
} from 'react-router-dom'

import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import {
  type AuthRedirectState,
  safeRedirect,
} from '../auth/navigation'
import { FormError, PageShell } from '../components/PageShell'

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
    <PageShell
      title="端对端加密备份"
      subtitle="使用邮箱和密码登录。主密钥只在本机 Worker 内解包。"
    >
      <form
        className="space-y-4"
        onSubmit={(event) => {
          event.preventDefault()
          void submit()
        }}
      >
        <label className="block text-sm font-medium text-slate-700">
          邮箱
          <input
            data-testid="login-email"
            type="email"
            required
            value={email}
            onChange={(event) => setEmail(event.target.value)}
            className="mt-1 w-full rounded-lg border border-slate-300 px-3 py-2 focus:border-slate-500 focus:outline-none"
          />
        </label>
        <label className="block text-sm font-medium text-slate-700">
          密码
          <input
            data-testid="login-password"
            type="password"
            required
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            className="mt-1 w-full rounded-lg border border-slate-300 px-3 py-2 focus:border-slate-500 focus:outline-none"
          />
        </label>
        <FormError message={error} />
        <button
          data-testid="login-submit"
          type="submit"
          disabled={busy}
          className="w-full rounded-lg bg-slate-900 px-4 py-2 text-sm font-semibold text-white disabled:opacity-50"
        >
          {busy ? '登录中…' : '登录'}
        </button>
      </form>
      <div className="mt-4 flex justify-between text-sm text-slate-600">
        <Link className="font-medium text-slate-900 underline" to="/register">
          注册
        </Link>
        <Link className="font-medium text-slate-900 underline" to="/reset">
          忘记密码 / 恢复密钥重置
        </Link>
      </div>
      <p className="mt-3 text-sm text-slate-600">
        <Link className="underline" to="/security">
          了解端对端加密与服务端可见信息
        </Link>
      </p>
    </PageShell>
  )
}
