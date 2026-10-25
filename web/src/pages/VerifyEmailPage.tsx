import { useEffect, useState } from 'react'
import { Link } from 'react-router-dom'

import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import { AuthLayout } from '../components/AuthLayout'
import { Button, FormError, FormNotice, Spinner, TextField } from '../ui'

type VerifyState = 'idle' | 'verifying' | 'success' | 'error' | 'missing'

export function VerifyEmailPage({
  token,
  onFinished,
  onDone,
}: {
  token: string | null
  onFinished: () => void
  onDone: () => void
}) {
  const { api } = useAuth()
  const [state, setState] = useState<VerifyState>(token ? 'verifying' : 'missing')
  const [error, setError] = useState<string | null>(null)
  const [resendEmail, setResendEmail] = useState('')
  const [resendMessage, setResendMessage] = useState<string | null>(null)

  useEffect(() => {
    if (!token || !api) {
      return
    }
    let cancelled = false
    void (async () => {
      try {
        await api.verifyEmail(token)
        if (!cancelled) {
          setState('success')
        }
      } catch (caught) {
        if (!cancelled) {
          setError(errorMessage(caught))
          setState('error')
        }
      } finally {
        if (!cancelled) {
          onFinished()
        }
      }
    })()
    return () => {
      cancelled = true
    }
  }, [api, onFinished, token])

  const resend = async () => {
    if (!api) {
      return
    }
    setError(null)
    setResendMessage(null)
    try {
      await api.resendVerification(resendEmail.trim())
      setResendMessage('如果该邮箱存在且未验证，我们已重新发送验证邮件。')
    } catch (caught) {
      setError(errorMessage(caught))
    }
  }

  return (
    <AuthLayout
      title={state === 'success' ? '邮箱已验证' : '验证邮箱'}
      subtitle="验证链接只能使用一次，请尽快完成验证。"
      footer={
        <Link to="/login" className="font-medium text-brand-600 hover:underline">
          返回登录
        </Link>
      }
    >
      <div className="space-y-4">
        {state === 'verifying' && (
          <div className="flex items-center justify-center gap-2 py-4 text-sm text-ink-600">
            <Spinner size={18} />
            正在验证…
          </div>
        )}
        {state === 'success' && (
          <FormNotice message="邮箱验证成功，现在可以登录了。" />
        )}
        {state === 'error' && (
          <>
            <FormError message={error} />
            <div className="space-y-3 rounded-card border border-line p-4">
              <p className="text-sm font-medium text-ink-900">
                重新发送验证邮件
              </p>
              <TextField
                aria-label="邮箱"
                type="email"
                autoComplete="email"
                value={resendEmail}
                onChange={(event) => setResendEmail(event.target.value)}
                placeholder="邮箱"
              />
              <Button onClick={() => void resend()}>重新发送</Button>
              <FormNotice message={resendMessage} />
            </div>
          </>
        )}
        {state === 'missing' && (
          <p className="text-sm leading-6 text-ink-600">
            缺少验证令牌，请从邮件中的链接重新打开。
          </p>
        )}
        {state !== 'verifying' && (
          <Button variant="primary" block onClick={onDone}>
            前往登录
          </Button>
        )}
      </div>
    </AuthLayout>
  )
}
