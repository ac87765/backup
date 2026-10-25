import { useState } from 'react'
import { Link } from 'react-router-dom'

import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import { FormError, FormSuccess, PageShell } from '../components/PageShell'
import { RecoveryConfirmation } from '../components/RecoveryConfirmation'

export function RotateRecoveryPage() {
  const { api } = useAuth()
  const [currentPassword, setCurrentPassword] = useState('')
  const [display, setDisplay] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [message, setMessage] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const prepare = async () => {
    setError(null)
    setMessage(null)
    if (!api) {
      setError('当前环境不支持 Worker')
      return
    }
    setBusy(true)
    try {
      const result = await api.prepareRecoveryRotation(currentPassword)
      setDisplay(result.recoveryDisplay)
      setCurrentPassword('')
    } catch (caught) {
      setError(errorMessage(caught))
    } finally {
      setBusy(false)
    }
  }

  if (display) {
    return (
      <PageShell
        title="保存新的恢复密钥"
        subtitle="确认后旧恢复密钥立即失效。"
      >
        <RecoveryConfirmation
          display={display}
          busy={busy}
          confirmLabel="确认并轮换"
          error={error}
          onConfirm={async () => {
            if (!api) {
              throw new Error('当前环境不支持 Worker')
            }
            setBusy(true)
            try {
              await api.confirmRecoveryRotation()
              setDisplay(null)
              setMessage('恢复密钥已轮换，旧恢复密钥已失效。')
            } catch (caught) {
              setDisplay(null)
              setError(errorMessage(caught))
            } finally {
              setBusy(false)
            }
          }}
          onCancel={() => {
            void api?.discardRecoveryRotation()
            setDisplay(null)
          }}
        />
      </PageShell>
    )
  }

  return (
    <PageShell
      title="轮换恢复密钥"
      subtitle="需要再次输入当前密码，防止遗留会话静默替换恢复材料。"
    >
      <form
        className="space-y-4"
        onSubmit={(event) => {
          event.preventDefault()
          void prepare()
        }}
      >
        <label className="block text-sm font-medium text-slate-700">
          当前密码
          <input
            data-testid="rotate-current-password"
            type="password"
            required
            value={currentPassword}
            onChange={(event) => setCurrentPassword(event.target.value)}
            className="mt-1 w-full rounded-lg border border-slate-300 px-3 py-2 focus:border-slate-500 focus:outline-none"
          />
        </label>
        <FormError message={error} />
        <FormSuccess message={message} />
        <button
          data-testid="rotate-prepare"
          type="submit"
          disabled={busy}
          className="w-full rounded-lg bg-slate-900 px-4 py-2 text-sm font-semibold text-white disabled:opacity-50"
        >
          {busy ? '准备中…' : '生成新的恢复密钥'}
        </button>
      </form>
      <Link className="mt-4 inline-block text-sm font-medium text-slate-900 underline" to="/">
        返回主页
      </Link>
    </PageShell>
  )
}
