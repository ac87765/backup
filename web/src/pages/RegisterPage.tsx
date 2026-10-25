import { useState } from 'react'
import { Link } from 'react-router-dom'

import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import { assessPassword } from '../auth/password-strength'
import { FormError, PageShell } from '../components/PageShell'
import { RecoveryConfirmation } from '../components/RecoveryConfirmation'

type Step = 'form' | 'recovery' | 'done'

export function RegisterPage() {
  const { api } = useAuth()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [step, setStep] = useState<Step>('form')
  const [recoveryDisplay, setRecoveryDisplay] = useState('')

  const submit = async () => {
    setError(null)
    const assessment = assessPassword(password)
    if (!assessment.ok) {
      setError(assessment.message ?? '密码强度不足')
      return
    }
    if (!api) {
      setError('当前环境不支持 Worker')
      return
    }
    setBusy(true)
    try {
      const result = await api.register(email.trim(), password)
      setRecoveryDisplay(result.recoveryDisplay)
      setStep('recovery')
    } catch (caught) {
      setError(errorMessage(caught))
    } finally {
      setBusy(false)
    }
  }

  if (step === 'recovery') {
    return (
      <PageShell
        title="保存恢复密钥"
        subtitle="它是忘记密码后恢复数据的唯一方式，请抄写并妥善保存。"
      >
        <RecoveryConfirmation
          display={recoveryDisplay}
          busy={busy}
          confirmLabel="确认已保存并注册"
          onConfirm={async () => {
            if (!api) {
              throw new Error('当前环境不支持 Worker')
            }
            setBusy(true)
            try {
              await api.confirmRegistration()
              setStep('done')
            } catch (caught) {
              setError(errorMessage(caught))
              setStep('form')
              setRecoveryDisplay('')
            } finally {
              setBusy(false)
            }
          }}
          onCancel={() => {
            void api?.discardRegistration()
            setStep('form')
            setRecoveryDisplay('')
          }}
        />
      </PageShell>
    )
  }

  if (step === 'done') {
    return (
      <PageShell title="注册成功" subtitle="请查收验证邮件，完成验证后即可登录。">
        <div className="space-y-4 text-sm text-slate-700">
          <p>
            验证邮件已发送到 <strong>{email}</strong>。邮件中的链接 24 小时内有效。
          </p>
          <Link className="font-medium text-slate-900 underline" to="/login">
            前往登录
          </Link>
        </div>
      </PageShell>
    )
  }

  return (
    <PageShell
      title="创建账号"
      subtitle="密码只在本机派生密钥，服务端永远不会收到明文密码。"
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
            data-testid="register-email"
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
            data-testid="register-password"
            type="password"
            required
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            className="mt-1 w-full rounded-lg border border-slate-300 px-3 py-2 focus:border-slate-500 focus:outline-none"
          />
          <span className="mt-1 block text-xs text-slate-500">
            至少 12 个字符，建议使用多个随机单词组成的短语。
          </span>
        </label>
        <FormError message={error} />
        <button
          data-testid="register-submit"
          type="submit"
          disabled={busy}
          className="w-full rounded-lg bg-slate-900 px-4 py-2 text-sm font-semibold text-white disabled:opacity-50"
        >
          {busy ? '准备中…' : '创建账号'}
        </button>
      </form>
      <p className="mt-4 text-sm text-slate-600">
        已有账号？{' '}
        <Link className="font-medium text-slate-900 underline" to="/login">
          登录
        </Link>
      </p>
    </PageShell>
  )
}
