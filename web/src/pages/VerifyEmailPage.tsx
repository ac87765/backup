import { useEffect, useState } from 'react'
import { Link } from 'react-router-dom'

import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import { FormError, FormSuccess, PageShell } from '../components/PageShell'

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
    <PageShell
      title={state === 'success' ? '邮箱已验证' : '验证邮箱'}
      subtitle="验证链接中的令牌只通过 POST 提交，并且只能使用一次。"
    >
      <div className="space-y-4">
        {state === 'verifying' && (
          <p role="status" className="text-sm text-slate-600">
            正在验证…
          </p>
        )}
        {state === 'success' && (
          <FormSuccess message="邮箱验证成功，现在可以登录了。" />
        )}
        {state === 'error' && (
          <>
            <FormError message={error} />
            <div className="space-y-2 rounded-lg border border-slate-200 p-4">
              <p className="text-sm font-medium text-slate-700">
                重新发送验证邮件
              </p>
              <input
                type="email"
                value={resendEmail}
                onChange={(event) => setResendEmail(event.target.value)}
                placeholder="邮箱"
                className="w-full rounded-lg border border-slate-300 px-3 py-2 text-sm focus:border-slate-500 focus:outline-none"
              />
              <button
                type="button"
                onClick={() => void resend()}
                className="rounded-lg border border-slate-300 px-3 py-2 text-sm font-medium text-slate-700"
              >
                重新发送
              </button>
              <FormSuccess message={resendMessage} />
            </div>
          </>
        )}
        {state === 'missing' && (
          <p className="text-sm text-slate-600">
            缺少验证令牌，请从邮件中的链接重新打开。
          </p>
        )}
        {state !== 'verifying' && (
          <button
            type="button"
            onClick={onDone}
            className="rounded-lg bg-slate-900 px-4 py-2 text-sm font-semibold text-white"
          >
            前往登录
          </button>
        )}
        <p className="text-sm text-slate-600">
          <Link className="font-medium text-slate-900 underline" to="/login">
            返回登录
          </Link>
        </p>
      </div>
    </PageShell>
  )
}
