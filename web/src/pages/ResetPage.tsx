import { useState } from 'react'
import { Link } from 'react-router-dom'

import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import { assessPassword } from '../auth/password-strength'
import { FormError, FormSuccess, PageShell } from '../components/PageShell'

export function ResetPage() {
  const { api } = useAuth()
  const [email, setEmail] = useState('')
  const [recoveryKey, setRecoveryKey] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [message, setMessage] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const submit = async () => {
    setError(null)
    setMessage(null)
    if (newPassword !== confirm) {
      setError('两次输入的新密码不一致')
      return
    }
    const assessment = assessPassword(newPassword)
    if (!assessment.ok) {
      setError(assessment.message ?? '新密码强度不足')
      return
    }
    if (!api) {
      setError('当前环境不支持 Worker')
      return
    }
    setBusy(true)
    try {
      // 先本地校验校验位/字符集：抄错时不发送任何 HTTP 请求。
      await api.validateRecoveryKey(recoveryKey)
      await api.resetWithRecovery(email.trim(), recoveryKey, newPassword)
      setMessage('密码已重置，所有旧会话已失效。请使用新密码登录。')
      setRecoveryKey('')
      setNewPassword('')
      setConfirm('')
    } catch (caught) {
      setError(errorMessage(caught))
    } finally {
      setBusy(false)
    }
  }

  return (
    <PageShell
      title="使用恢复密钥重置密码"
      subtitle="恢复密钥只在本机 Worker 内解包 MK，服务端只收到恢复认证值。"
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
            data-testid="reset-email"
            type="email"
            required
            value={email}
            onChange={(event) => setEmail(event.target.value)}
            className="mt-1 w-full rounded-lg border border-slate-300 px-3 py-2 focus:border-slate-500 focus:outline-none"
          />
        </label>
        <label className="block text-sm font-medium text-slate-700">
          恢复密钥
          <input
            data-testid="reset-recovery-key"
            type="text"
            required
            value={recoveryKey}
            onChange={(event) => setRecoveryKey(event.target.value)}
            autoComplete="off"
            spellCheck={false}
            className="mt-1 w-full rounded-lg border border-slate-300 px-3 py-2 font-mono uppercase tracking-wide focus:border-slate-500 focus:outline-none"
          />
        </label>
        <label className="block text-sm font-medium text-slate-700">
          新密码
          <input
            data-testid="reset-new-password"
            type="password"
            required
            value={newPassword}
            onChange={(event) => setNewPassword(event.target.value)}
            className="mt-1 w-full rounded-lg border border-slate-300 px-3 py-2 focus:border-slate-500 focus:outline-none"
          />
        </label>
        <label className="block text-sm font-medium text-slate-700">
          确认新密码
          <input
            data-testid="reset-confirm-password"
            type="password"
            required
            value={confirm}
            onChange={(event) => setConfirm(event.target.value)}
            className="mt-1 w-full rounded-lg border border-slate-300 px-3 py-2 focus:border-slate-500 focus:outline-none"
          />
        </label>
        <FormError message={error} />
        <FormSuccess message={message} />
        <button
          data-testid="reset-submit"
          type="submit"
          disabled={busy}
          className="w-full rounded-lg bg-slate-900 px-4 py-2 text-sm font-semibold text-white disabled:opacity-50"
        >
          {busy ? '重置中…' : '重置密码'}
        </button>
      </form>
      <div className="mt-4 flex justify-between text-sm text-slate-600">
        <Link className="font-medium text-slate-900 underline" to="/login">
          返回登录
        </Link>
        <span>同时丢失密码与恢复密钥时，数据无法恢复。</span>
      </div>
    </PageShell>
  )
}
