import { useState } from 'react'
import { Link } from 'react-router-dom'

import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import { assessPassword } from '../auth/password-strength'
import { FormError, FormSuccess, PageShell } from '../components/PageShell'

export function ChangePasswordPage() {
  const { api } = useAuth()
  const [oldPassword, setOldPassword] = useState('')
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
      await api.changePassword(oldPassword, newPassword)
      setOldPassword('')
      setNewPassword('')
      setConfirm('')
      setMessage('密码已修改，其他设备的会话已失效。')
    } catch (caught) {
      setError(errorMessage(caught))
    } finally {
      setBusy(false)
    }
  }

  return (
    <PageShell title="修改密码" subtitle="文件密文不会重新加密；只替换认证材料与包裹后的 MK。">
      <form
        className="space-y-4"
        onSubmit={(event) => {
          event.preventDefault()
          void submit()
        }}
      >
        <label className="block text-sm font-medium text-slate-700">
          旧密码
          <input
            data-testid="change-old-password"
            type="password"
            required
            value={oldPassword}
            onChange={(event) => setOldPassword(event.target.value)}
            className="mt-1 w-full rounded-lg border border-slate-300 px-3 py-2 focus:border-slate-500 focus:outline-none"
          />
        </label>
        <label className="block text-sm font-medium text-slate-700">
          新密码
          <input
            data-testid="change-new-password"
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
            data-testid="change-confirm-password"
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
          data-testid="change-submit"
          type="submit"
          disabled={busy}
          className="w-full rounded-lg bg-slate-900 px-4 py-2 text-sm font-semibold text-white disabled:opacity-50"
        >
          {busy ? '修改中…' : '修改密码'}
        </button>
      </form>
      <Link className="mt-4 inline-block text-sm font-medium text-slate-900 underline" to="/">
        返回主页
      </Link>
    </PageShell>
  )
}
