import { useState } from 'react'
import { Link } from 'react-router-dom'

import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import { assessPassword } from '../auth/password-strength'
import { AuthLayout } from '../components/AuthLayout'
import { RecoveryConfirmation } from '../components/RecoveryConfirmation'
import { Button, FormError, TextField } from '../ui'

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
      setError('当前浏览器不支持加密功能')
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
      <AuthLayout
        title="保存恢复密钥"
        subtitle="它是忘记密码后恢复数据的唯一方式，请抄写并妥善保存。"
        size="lg"
      >
        <RecoveryConfirmation
          display={recoveryDisplay}
          busy={busy}
          confirmLabel="确认已保存并注册"
          onConfirm={async () => {
            if (!api) {
              throw new Error('当前浏览器不支持加密功能')
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
      </AuthLayout>
    )
  }

  if (step === 'done') {
    return (
      <AuthLayout
        title="注册成功"
        subtitle="请查收验证邮件，完成验证后即可登录。"
      >
        <div className="space-y-4 text-sm leading-6 text-ink-600">
          <p>
            验证邮件已发送到{' '}
            <strong className="break-all text-ink-900">{email}</strong>
            。邮件中的链接 24 小时内有效。
          </p>
          <Link
            to="/login"
            className="inline-flex font-medium text-brand-600 hover:underline"
          >
            前往登录
          </Link>
        </div>
      </AuthLayout>
    )
  }

  return (
    <AuthLayout
      title="创建账号"
      subtitle="注册后即可在浏览器里加密备份文件，服务端看不到文件内容。"
      footer={
        <p>
          已有账号？{' '}
          <Link to="/login" className="font-medium text-brand-600 hover:underline">
            登录
          </Link>
        </p>
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
          data-testid="register-email"
          type="email"
          required
          autoComplete="email"
          value={email}
          onChange={(event) => setEmail(event.target.value)}
        />
        <TextField
          label="密码"
          data-testid="register-password"
          type="password"
          required
          autoComplete="new-password"
          hint="至少 12 个字符，建议使用多个随机单词组成的短语。"
          value={password}
          onChange={(event) => setPassword(event.target.value)}
        />
        <Button
          type="submit"
          variant="primary"
          block
          data-testid="register-submit"
          disabled={busy}
        >
          {busy ? '准备中…' : '创建账号'}
        </Button>
      </form>
    </AuthLayout>
  )
}
