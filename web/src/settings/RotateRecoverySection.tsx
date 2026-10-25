import { useState } from 'react'

import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import { RecoveryConfirmation } from '../components/RecoveryConfirmation'
import { Button, FormError, TextField, useToast } from '../ui'

export function RotateRecoverySection() {
  const { api } = useAuth()
  const { showToast } = useToast()
  const [currentPassword, setCurrentPassword] = useState('')
  const [display, setDisplay] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const prepare = async () => {
    setError(null)
    if (!api) {
      setError('当前浏览器不支持加密功能')
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
      <div className="space-y-4">
        <div className="space-y-1">
          <h2 className="text-base font-semibold text-ink-900">
            保存新的恢复密钥
          </h2>
          <p className="text-sm leading-6 text-ink-600">
            确认后旧恢复密钥立即失效。
          </p>
        </div>
        <RecoveryConfirmation
          display={display}
          busy={busy}
          confirmLabel="确认并轮换"
          error={error}
          onConfirm={async () => {
            if (!api) {
              throw new Error('当前浏览器不支持加密功能')
            }
            setBusy(true)
            try {
              await api.confirmRecoveryRotation()
              setDisplay(null)
              showToast('恢复密钥已轮换，旧恢复密钥已失效。')
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
      </div>
    )
  }

  return (
    <form
      className="max-w-md space-y-4"
      onSubmit={(event) => {
        event.preventDefault()
        void prepare()
      }}
    >
      <p className="text-sm leading-6 text-ink-600">
        需要再次输入当前密码，以防他人替换你的恢复密钥。
      </p>
      <TextField
        label="当前密码"
        data-testid="rotate-current-password"
        type="password"
        required
        autoComplete="current-password"
        value={currentPassword}
        onChange={(event) => setCurrentPassword(event.target.value)}
      />
      <FormError message={error} />
      <Button
        type="submit"
        variant="primary"
        data-testid="rotate-prepare"
        disabled={busy}
      >
        {busy ? '准备中…' : '生成新的恢复密钥'}
      </Button>
    </form>
  )
}
