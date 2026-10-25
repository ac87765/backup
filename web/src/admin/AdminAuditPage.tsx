import { useEffect, useState } from 'react'
import { useSearchParams } from 'react-router-dom'
import { ClipboardList, Filter } from 'lucide-react'

import { Button, Spinner, TextField, useToast } from '../ui'
import {
  AdminApiError,
  adminApi,
  type AdminAuditResponse,
} from './api'
import { AdminErrorBanner, AdminReauthNotice } from './AdminLayout'
import { formatDateTime } from './format'

const ACTION_LABELS: Record<string, string> = {
  approve: '批准账号',
  reject: '拒绝账号',
  suspend: '停用账号',
  reactivate: '恢复账号',
  update_quota: '修改额度',
  revoke_sessions: '注销会话',
  update_settings: '修改设置',
  grant_admin: '授予管理员（命令行）',
  revoke_admin: '撤销管理员（命令行）',
}

export function AdminAuditPage() {
  const [searchParams, setSearchParams] = useSearchParams()
  const target = searchParams.get('target_user_id') ?? ''
  const [targetInput, setTargetInput] = useState(target)
  const [items, setItems] = useState<AdminAuditResponse['items']>([])
  const [cursor, setCursor] = useState<number | null>(null)
  const [loading, setLoading] = useState(true)
  const [loadingMore, setLoadingMore] = useState(false)
  const [error, setError] = useState<unknown>(null)
  const { showToast } = useToast()

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const response = await adminApi.listAudit({
          targetUserId: target || undefined,
          limit: 50,
        })
        if (!cancelled) {
          setItems(response.items)
          setCursor(response.next_cursor ?? null)
          setError(null)
          setLoading(false)
        }
      } catch (caught) {
        if (!cancelled) {
          setError(caught)
          setLoading(false)
        }
      }
    })()
    return () => {
      cancelled = true
    }
  }, [target])

  const loadMore = async () => {
    if (!cursor || loadingMore) {
      return
    }
    setLoadingMore(true)
    try {
      const response = await adminApi.listAudit({
        targetUserId: target || undefined,
        cursor: String(cursor),
        limit: 50,
      })
      setItems((current) => [...current, ...response.items])
      setCursor(response.next_cursor ?? null)
    } catch {
      showToast('加载更多审计失败', 'error')
    } finally {
      setLoadingMore(false)
    }
  }

  if (error instanceof AdminApiError && error.code === 'reauth_required') {
    return <AdminReauthNotice />
  }

  return (
    <div data-testid="admin-audit" className="space-y-5">
      <header>
        <h1 className="text-2xl font-semibold text-ink-900">审计日志</h1>
        <p className="mt-1 text-sm text-ink-600">
          每一次管理操作都会在同一事务内留下只追加的记录。日志不包含任何文件信息。
        </p>
      </header>

      <form
        className="flex flex-wrap items-end gap-2"
        onSubmit={(event) => {
          event.preventDefault()
          if (targetInput.trim()) {
            setLoading(true)
            setSearchParams({ target_user_id: targetInput.trim() })
          } else {
            setLoading(true)
            setSearchParams({})
          }
        }}
      >
        <TextField
          label="按用户 ID 筛选"
          data-testid="admin-audit-target"
          placeholder="用户 UUID（留空查看全部）"
          value={targetInput}
          onChange={(event) => setTargetInput(event.target.value)}
          leading={<Filter size={15} />}
          containerClassName="max-w-md"
        />
        <Button type="submit" variant="secondary">
          筛选
        </Button>
        {target && (
          <Button
            variant="ghost"
            onClick={() => {
              setTargetInput('')
              setLoading(true)
              setSearchParams({})
            }}
          >
            清除筛选
          </Button>
        )}
      </form>

      <AdminErrorBanner error={error} />

      {loading ? (
        <div className="flex justify-center py-16">
          <Spinner size={26} label="加载审计日志" />
        </div>
      ) : items.length === 0 ? (
        <div className="rounded-card border border-line bg-white px-4 py-16 text-center">
          <ClipboardList
            size={26}
            className="mx-auto mb-3 text-ink-400"
            aria-hidden="true"
          />
          <p className="text-sm text-ink-600">暂无审计记录。</p>
        </div>
      ) : (
        <ol className="space-y-3">
          {items.map((entry) => (
            <li
              key={entry.id}
              data-testid="admin-audit-row"
              className="rounded-card border border-line bg-white p-4"
            >
              <div className="flex flex-wrap items-center gap-2">
                <span className="rounded-full bg-brand-50 px-2 py-0.5 text-xs font-medium text-brand-600">
                  {ACTION_LABELS[entry.action] ?? entry.action}
                </span>
                <span className="text-xs text-ink-500">
                  {formatDateTime(entry.created_at)} · {entry.ip}
                </span>
              </div>
              <p className="mt-2 text-xs text-ink-600">
                操作者 <code className="break-all">{entry.admin_id}</code>
                {entry.target_user_id && (
                  <>
                    {' '}→ 目标{' '}
                    <code className="break-all">{entry.target_user_id}</code>
                  </>
                )}
              </p>
              <pre className="mt-2 overflow-x-auto whitespace-pre-wrap break-all rounded bg-canvas p-2 text-[11px] leading-5 text-ink-600">
                {JSON.stringify(entry.details, null, 2)}
              </pre>
            </li>
          ))}
        </ol>
      )}

      {cursor && (
        <div className="flex justify-center">
          <Button
            variant="secondary"
            disabled={loadingMore}
            onClick={() => void loadMore()}
          >
            {loadingMore ? '加载中…' : '加载更多'}
          </Button>
        </div>
      )}
    </div>
  )
}
