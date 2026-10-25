import { useEffect, useState } from 'react'
import { Monitor } from 'lucide-react'

import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import { Button, EmptyState, FormError, useToast } from '../ui'
import type { SessionSummaryView } from '../worker/types'

export function SessionsSection() {
  const { api } = useAuth()
  const { showToast } = useToast()
  const [sessions, setSessions] = useState<SessionSummaryView[]>([])
  const [error, setError] = useState<string | null>(null)
  const [busyId, setBusyId] = useState<string | null>(null)

  useEffect(() => {
    if (!api) {
      return
    }
    let cancelled = false
    void (async () => {
      try {
        const rows = await api.getSessions()
        if (!cancelled) {
          setSessions(rows)
          setError(null)
        }
      } catch (caught) {
        if (!cancelled) {
          setError(errorMessage(caught))
        }
      }
    })()
    return () => {
      cancelled = true
    }
  }, [api])

  const revoke = async (sessionId: string) => {
    if (!api) {
      return
    }
    setBusyId(sessionId)
    try {
      await api.revokeSession(sessionId)
      setSessions(await api.getSessions())
      showToast('已撤销该登录设备')
    } catch (caught) {
      showToast(errorMessage(caught), 'error')
    } finally {
      setBusyId(null)
    }
  }

  return (
    <div className="space-y-4">
      <p className="text-sm leading-6 text-ink-600">
        会话空闲 7 天或绝对存在 30 天后自动失效。
      </p>
      <FormError message={error} />
      {sessions.length === 0 ? (
        <EmptyState
          icon={<Monitor size={24} />}
          title="没有活跃会话"
          description="当前账号还没有其他登录设备。"
        />
      ) : (
        <ul className="divide-y divide-line rounded-card border border-line">
          {sessions.map((session) => (
            <li
              key={session.id}
              className="flex flex-wrap items-center justify-between gap-3 px-4 py-4"
            >
              <div className="min-w-0 space-y-1">
                <p className="flex items-center gap-2 text-sm font-medium text-ink-900">
                  {session.current ? '当前设备' : '其他设备'}
                  {session.current && (
                    <span className="rounded-full bg-brand-50 px-2 py-0.5 text-xs font-medium text-brand-600">
                      本机
                    </span>
                  )}
                </p>
                <p className="text-xs leading-5 text-ink-600">
                  IP {session.ip} · 最近活动{' '}
                  {new Date(session.lastSeenAt).toLocaleString('zh-CN')}
                </p>
                <p className="text-xs leading-5 text-ink-600">
                  到期 {new Date(session.expiresAt).toLocaleString('zh-CN')}
                </p>
                {session.userAgent && (
                  <p className="break-all text-xs leading-5 text-ink-500">
                    {session.userAgent}
                  </p>
                )}
              </div>
              {!session.current && (
                <Button
                  size="sm"
                  variant="secondary"
                  disabled={busyId === session.id}
                  onClick={() => void revoke(session.id)}
                >
                  撤销
                </Button>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
