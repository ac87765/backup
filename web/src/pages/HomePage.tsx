import { useState } from 'react'
import { Link, useNavigate } from 'react-router-dom'

import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import { FormError, FormSuccess, PageShell } from '../components/PageShell'

export function HomePage() {
  const { api, user, logout } = useAuth()
  const navigate = useNavigate()
  const [message, setMessage] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  if (!user) {
    return null
  }

  const resend = async () => {
    setError(null)
    setMessage(null)
    if (!api) {
      return
    }
    try {
      await api.resendVerification(user.email)
      setMessage('如果该邮箱尚未验证，我们已重新发送验证邮件。')
    } catch (caught) {
      setError(errorMessage(caught))
    }
  }

  return (
    <PageShell
      title="端对端加密备份"
      subtitle="账号已就绪。文件浏览器将在第 7 组接入。"
    >
      <div className="space-y-4 text-sm text-slate-700">
        <p>
          当前账号：<strong data-testid="home-email">{user.email}</strong>
        </p>
        <p data-testid="home-verified">
          邮箱状态：{user.emailVerified ? '已验证' : '待验证'}
        </p>
        {!user.emailVerified && (
          <div className="space-y-2 rounded-lg border border-amber-200 bg-amber-50 p-3">
            <p>请查收验证邮件后再上传文件。</p>
            <button
              type="button"
              onClick={() => void resend()}
              className="rounded-lg border border-amber-300 px-3 py-2 font-medium text-amber-900"
            >
              重新发送验证邮件
            </button>
            <FormSuccess message={message} />
            <FormError message={error} />
          </div>
        )}
        <nav className="grid gap-2">
          <Link className="underline" to="/sessions">
            会话管理
          </Link>
          <Link className="underline" to="/change-password">
            修改密码
          </Link>
          <Link className="underline" to="/rotate-recovery-key">
            轮换恢复密钥
          </Link>
        </nav>
        <button
          type="button"
          onClick={() => {
            void logout().catch(() => undefined).finally(() => { void navigate('/login') })
          }}
          className="rounded-lg bg-slate-900 px-4 py-2 font-semibold text-white"
        >
          退出登录
        </button>
      </div>
    </PageShell>
  )
}
