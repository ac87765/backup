import { useState } from 'react'

import {
  pickChallengeIndices,
  recoveryGroups,
  verifyRecoveryFragments,
} from '../auth/recovery'
import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import { Button, FormError, TextField } from '../ui'

interface RecoveryConfirmationProps {
  display: string
  error?: string | null
  busy?: boolean
  confirmLabel?: string
  onConfirm: () => Promise<void> | void
  onCancel?: () => void
}

/** 展示恢复密钥，并要求回填随机抽取的若干组。 */
export function RecoveryConfirmation({
  display,
  error,
  busy,
  confirmLabel = '确认并继续',
  onConfirm,
  onCancel,
}: RecoveryConfirmationProps) {
  const { api } = useAuth()
  const groups = recoveryGroups(display)
  const [indices] = useState(() => pickChallengeIndices(groups.length, 3))
  const [answers, setAnswers] = useState<string[]>(() =>
    indices.map(() => ''),
  )
  const [localError, setLocalError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)

  const updateAnswer = (position: number, value: string) => {
    setAnswers((current) =>
      current.map((answer, index) => (index === position ? value : answer)),
    )
  }

  const submit = async () => {
    setLocalError(null)
    if (!verifyRecoveryFragments(display, indices, answers)) {
      setLocalError('回填的片段与恢复密钥不一致，请重新核对')
      return
    }
    if (!api) {
      setLocalError('当前环境不支持 Worker')
      return
    }
    setSubmitting(true)
    try {
      await api.validateRecoveryKey(display)
      await onConfirm()
    } catch (caught) {
      setLocalError(errorMessage(caught))
    } finally {
      setSubmitting(false)
    }
  }

  return (
    <div className="space-y-5">
      <div className="space-y-2">
        <p className="text-sm font-medium text-ink-900">请抄写恢复密钥</p>
        <code
          data-testid="recovery-display"
          className="block break-all rounded-control border border-line bg-canvas p-4 font-mono text-sm leading-6 tracking-wide text-ink-900"
        >
          {display}
        </code>
      </div>

      <div className="space-y-3">
        <p className="text-sm leading-6 text-ink-600">
          请回填以下组以确认已保存（不区分大小写）：
        </p>
        {indices.map((groupIndex, position) => (
          <TextField
            key={groupIndex}
            label={`第 ${groupIndex + 1} 组`}
            data-testid={`recovery-confirm-${position}`}
            data-group-index={groupIndex}
            value={answers[position] ?? ''}
            onChange={(event) => updateAnswer(position, event.target.value)}
            autoComplete="off"
            spellCheck={false}
            inputClassName="font-mono uppercase tracking-wide"
          />
        ))}
      </div>

      <FormError message={localError ?? error ?? null} />

      <div className="flex flex-wrap gap-2">
        <Button
          type="button"
          variant="primary"
          onClick={() => void submit()}
          disabled={busy || submitting}
        >
          {submitting ? '校验中…' : confirmLabel}
        </Button>
        {onCancel && (
          <Button
            type="button"
            onClick={onCancel}
            disabled={busy || submitting}
          >
            取消
          </Button>
        )}
      </div>
    </div>
  )
}
