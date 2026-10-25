import { useState } from 'react'

import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import { assessPassword } from '../auth/password-strength'
import { Button, FormError, TextField, useToast } from '../ui'

export function ChangePasswordSection() {
  const { api } = useAuth()
  const { showToast } = useToast()
  const [oldPassword, setOldPassword] = useState('')
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
      setError('当前环境不支持 Worker')
      return
    }
    setBusy(true)
    try {
      await api.changePassword(oldPassword, newPassword)
      setOldPassword('')
      setNewPassword('')
      setConfirm('')
      showToast('密码已修改，其他设备的会话已失效。')
    } catch (caught) {
      setError(errorMessage(caught))
    } finally {
      setBusy(false)
    }
  }

  return (
    <form
      className="max-w-md space-y-4"
      onSubmit={(event) => {
        event.preventDefault()
        void submit()
      }}
    >
      <p className="text-sm leading-6 text-ink-600">
        文件密文不会重新加密；只替换认证材料与包裹后的 MK。
      </p>
      <TextField
        label="旧密码"
        data-testid="change-old-password"
        type="password"
        required
        autoComplete="current-password"
        value={oldPassword}
        onChange={(event) => setOldPassword(event.target.value)}
      />
      <TextField
        label="新密码"
        data-testid="change-new-password"
        type="password"
        required
        autoComplete="new-password"
        hint="至少 12 个字符，建议使用多个随机单词组成的短语。"
        value={newPassword}
        onChange={(event) => setNewPassword(event.target.value)}
      />
      <TextField
        label="确认新密码"
        data-testid="change-confirm-password"
        type="password"
        required
        autoComplete="new-password"
        value={confirm}
        onChange={(event) => setConfirm(event.target.value)}
      />
      <FormError message={error} />
      <Button type="submit" variant="primary" disabled={busy}>
        {busy ? '修改中…' : '修改密码'}
      </Button>
    </form>
  )
}
