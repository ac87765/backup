import { useState } from 'react'
import { Link } from 'react-router-dom'

import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import { assessPassword } from '../auth/password-strength'
import { AuthLayout } from '../components/AuthLayout'
import { Button, FormError, TextField, useToast } from '../ui'

export function ResetPage() {
  const { api } = useAuth()
  const { showToast } = useToast()
  const [email, setEmail] = useState('')
  const [recoveryKey, setRecoveryKey] = useState('')
  const [newPassword, setNewPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  const submit = async () => {
    setError(null)
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
      setError('当前浏览器不支持加密功能')
      return
    }
    setBusy(true)
    try {
      // 先本地校验校验位/字符集：抄错时不发送任何 HTTP 请求。
      await api.validateRecoveryKey(recoveryKey)
      await api.resetWithRecovery(email.trim(), recoveryKey, newPassword)
      showToast('密码已重置，所有旧会话已失效。请使用新密码登录。')
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
    <AuthLayout
      title="使用恢复密钥重置密码"
      subtitle="输入恢复密钥和新密码；恢复密钥只在本机校验，不会发送到服务端。"
      footer={
        <div className="space-y-1.5">
          <p>
            <Link to="/login" className="font-medium text-brand-600 hover:underline">
              返回登录
            </Link>
          </p>
          <p className="text-xs text-ink-500">
            同时丢失密码与恢复密钥时，数据无法恢复。
          </p>
        </div>
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
          data-testid="reset-email"
          type="email"
          required
          autoComplete="email"
          value={email}
          onChange={(event) => setEmail(event.target.value)}
        />
        <TextField
          label="恢复密钥"
          data-testid="reset-recovery-key"
          type="text"
          required
          autoComplete="off"
          spellCheck={false}
          value={recoveryKey}
          onChange={(event) => setRecoveryKey(event.target.value)}
          inputClassName="font-mono uppercase tracking-wide"
        />
        <TextField
          label="新密码"
          data-testid="reset-new-password"
          type="password"
          required
          autoComplete="new-password"
          value={newPassword}
          onChange={(event) => setNewPassword(event.target.value)}
        />
        <TextField
          label="确认新密码"
          data-testid="reset-confirm-password"
          type="password"
          required
          autoComplete="new-password"
          value={confirm}
          onChange={(event) => setConfirm(event.target.value)}
        />
        <Button
          type="submit"
          variant="primary"
          block
          data-testid="reset-submit"
          disabled={busy}
        >
          {busy ? '重置中…' : '重置密码'}
        </Button>
      </form>
    </AuthLayout>
  )
}
