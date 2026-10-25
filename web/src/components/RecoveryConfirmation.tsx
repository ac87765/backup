import { useState } from 'react'

import {
  pickChallengeIndices,
  recoveryGroups,
  verifyRecoveryFragments,
} from '../auth/recovery'
import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'

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
    <div className="space-y-4">
      <div>
        <p className="text-sm font-medium text-slate-700">请抄写恢复密钥</p>
        <code
          data-testid="recovery-display"
          className="mt-2 block break-all rounded-lg bg-slate-100 p-4 font-mono text-sm tracking-wide text-slate-900"
        >
          {display}
        </code>
      </div>

      <div className="space-y-3">
        <p className="text-sm text-slate-600">
          请回填以下组以确认已保存（不区分大小写）：
        </p>
        {indices.map((groupIndex, position) => (
          <label key={groupIndex} className="block text-sm font-medium text-slate-700">
            第 {groupIndex + 1} 组
            <input
              data-testid={`recovery-confirm-${position}`}
              data-group-index={groupIndex}
              value={answers[position] ?? ''}
              onChange={(event) => updateAnswer(position, event.target.value)}
              autoComplete="off"
              spellCheck={false}
              className="mt-1 w-full rounded-lg border border-slate-300 px-3 py-2 font-mono uppercase tracking-wide focus:border-slate-500 focus:outline-none"
            />
          </label>
        ))}
      </div>

      {(localError || error) && (
        <p role="alert" className="rounded-lg bg-red-50 p-3 text-sm text-red-700">
          {localError ?? error}
        </p>
      )}

      <div className="flex gap-3">
        <button
          type="button"
          onClick={() => void submit()}
          disabled={busy || submitting}
          className="rounded-lg bg-slate-900 px-4 py-2 text-sm font-semibold text-white disabled:opacity-50"
        >
          {submitting ? '校验中…' : confirmLabel}
        </button>
        {onCancel && (
          <button
            type="button"
            onClick={onCancel}
            disabled={busy || submitting}
            className="rounded-lg border border-slate-300 px-4 py-2 text-sm font-medium text-slate-700 disabled:opacity-50"
          >
            取消
          </button>
        )}
      </div>
    </div>
  )
}
