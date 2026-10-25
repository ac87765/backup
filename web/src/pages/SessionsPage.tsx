import { useCallback, useEffect, useState } from 'react'
import { Link } from 'react-router-dom'

import { useAuth } from '../auth/context'
import { errorMessage } from '../auth/errors'
import { FormError, PageShell } from '../components/PageShell'
import type { SessionSummaryView } from '../worker/types'

export function SessionsPage() {
  const { api } = useAuth()
  const [sessions, setSessions] = useState<SessionSummaryView[]>([])
  const [error, setError] = useState<string | null>(null)
  const [busyId, setBusyId] = useState<string | null>(null)

  const load = useCallback(async () => {
    if (!api) {
      return
    }
    try {
      setSessions(await api.getSessions())
    } catch (caught) {
      setError(errorMessage(caught))
    }
  }, [api])

  useEffect(() => {
    let cancelled = false
    void (async () => {
      if (!api) {
        return
      }
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
      await load()
    } catch (caught) {
      setError(errorMessage(caught))
    } finally {
      setBusyId(null)
    }
  }

  return (
    <PageShell title="会话管理" subtitle="会话空闲 7 天或绝对存在 30 天后自动失效。">
      <div className="space-y-3">
        <FormError message={error} />
        {sessions.map((session) => (
          <article
            key={session.id}
            className="rounded-xl border border-slate-200 p-3 text-sm text-slate-700"
          >
            <div className="flex items-center justify-between gap-3">
              <div>
                <p className="font-medium text-slate-900">
                  {session.current ? '当前设备' : '其他设备'}
                </p>
                <p className="text-xs text-slate-500">
                  IP {session.ip} · 最近活动{' '}
                  {new Date(session.lastSeenAt).toLocaleString()}
                </p>
                <p className="text-xs text-slate-500">
                  到期 {new Date(session.expiresAt).toLocaleString()}
                </p>
                {session.userAgent && (
                  <p className="mt-1 break-all text-xs text-slate-500">
                    {session.userAgent}
                  </p>
                )}
              </div>
              {!session.current && (
                <button
                  type="button"
                  disabled={busyId === session.id}
                  onClick={() => void revoke(session.id)}
                  className="rounded-lg border border-red-200 px-3 py-2 text-xs font-medium text-red-700 disabled:opacity-50"
                >
                  撤销
                </button>
              )}
            </div>
          </article>
        ))}
        {sessions.length === 0 && (
          <p className="text-sm text-slate-600">没有活跃会话。</p>
        )}
        <Link className="text-sm font-medium text-slate-900 underline" to="/">
          返回主页
        </Link>
      </div>
    </PageShell>
  )
}
