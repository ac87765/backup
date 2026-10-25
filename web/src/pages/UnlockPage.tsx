import { useState } from 'react'
import { Navigate, useNavigate } from 'react-router-dom'

import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import { FormError, PageShell } from '../components/PageShell'

export function UnlockPage() {
  const { api, user, loading, unlocked, markUnlocked, logout } = useAuth()
  const navigate = useNavigate()
  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  if (loading) {
    return (
      <PageShell title="解锁" subtitle="正在检查会话…">
        <p role="status" className="text-sm text-slate-600">
          加载中…
        </p>
      </PageShell>
    )
  }
  if (!user) {
    return <Navigate to="/login" replace />
  }
  if (unlocked) {
    return <Navigate to="/" replace />
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
      void navigate('/')
    } catch (caught) {
      setError(errorMessage(caught))
    } finally {
      setBusy(false)
    }
  }

  return (
    <PageShell
      title="解锁"
      subtitle={`已登录为 ${user.email}。主密钥只存在于本页 Worker 内存，刷新后需要重新解锁。`}
    >
      <form
        className="space-y-4"
        onSubmit={(event) => {
          event.preventDefault()
          void submit()
        }}
      >
        <label className="block text-sm font-medium text-slate-700">
          密码
          <input
            data-testid="unlock-password"
            type="password"
            required
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            className="mt-1 w-full rounded-lg border border-slate-300 px-3 py-2 focus:border-slate-500 focus:outline-none"
          />
        </label>
        <FormError message={error} />
        <button
          data-testid="unlock-submit"
          type="submit"
          disabled={busy}
          className="w-full rounded-lg bg-slate-900 px-4 py-2 text-sm font-semibold text-white disabled:opacity-50"
        >
          {busy ? '解锁中…' : '解锁'}
        </button>
      </form>
      <button
        type="button"
        onClick={() => {
          void logout().finally(() => { void navigate('/login') })
        }}
        className="mt-4 text-sm font-medium text-slate-600 underline"
      >
        退出登录
      </button>
    </PageShell>
  )
}
