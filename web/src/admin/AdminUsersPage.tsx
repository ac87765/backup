import { useCallback, useEffect, useMemo, useState } from 'react'
import { Link, useSearchParams } from 'react-router-dom'
import {
  Check,
  ChevronRight,
  Search,
  ShieldOff,
  UserCheck,
  UserX,
  X,
} from 'lucide-react'

import { errorMessage } from '../auth/errors'
import { formatBytes } from '../files/format'
import { VirtualList } from '../files/VirtualList'
import {
  Button,
  cx,
  Dialog,
  ProgressBar,
  Spinner,
  TextField,
  useMediaQuery,
  useToast,
} from '../ui'
import {
  AdminApiError,
  adminApi,
  type AdminOverview,
  type AdminUserDetailResponse,
  type AdminUserSummary,
} from './api'
import { AdminErrorBanner, AdminReauthNotice } from './AdminLayout'
import {
  GIB,
  QUOTA_PRESETS_GB,
  STATUS_LABELS,
  STATUS_TONES,
  bytesToGb,
  formatDateTime,
  gbToBytes,
} from './format'

const TABS = [
  { key: '', label: '全部' },
  { key: 'pending', label: '待审核' },
  { key: 'active', label: '正常' },
  { key: 'suspended', label: '已停用' },
  { key: 'rejected', label: '已拒绝' },
] as const

function isReauth(error: unknown): boolean {
  return error instanceof AdminApiError && error.code === 'reauth_required'
}

function StatusBadge({ status }: { status: string }) {
  return (
    <span
      className={cx(
        'inline-flex rounded-full px-2 py-0.5 text-xs font-medium',
        STATUS_TONES[status] ?? 'bg-canvas-strong text-ink-600',
      )}
    >
      {STATUS_LABELS[status] ?? status}
    </span>
  )
}

export function AdminUsersPage() {
  const [searchParams, setSearchParams] = useSearchParams()
  const statusFilter = searchParams.get('status') ?? ''
  const [searchInput, setSearchInput] = useState('')
  const [search, setSearch] = useState('')
  const [users, setUsers] = useState<AdminUserSummary[]>([])
  const [cursor, setCursor] = useState<string | null>(null)
  const [overview, setOverview] = useState<AdminOverview | null>(null)
  const [loading, setLoading] = useState(true)
  const [loadingMore, setLoadingMore] = useState(false)
  const [error, setError] = useState<unknown>(null)
  const [refreshKey, setRefreshKey] = useState(0)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [approveTarget, setApproveTarget] = useState<AdminUserSummary | null>(null)
  const [danger, setDanger] = useState<{
    kind: 'reject' | 'suspend'
    user: AdminUserSummary
  } | null>(null)

  const { showToast } = useToast()

  useEffect(() => {
    const timer = setTimeout(() => setSearch(searchInput.trim()), 300)
    return () => clearTimeout(timer)
  }, [searchInput])

  const reload = useCallback(() => setRefreshKey((value) => value + 1), [])

  useEffect(() => {
    let cancelled = false
    void (async () => {
      setLoading(true)
      setError(null)
      try {
        const response = await adminApi.listUsers({
          status: statusFilter || undefined,
          q: search || undefined,
          limit: 50,
        })
        if (!cancelled) {
          setUsers(response.items)
          setCursor(response.next_cursor ?? null)
        }
      } catch (caught) {
        if (!cancelled) {
          setError(caught)
        }
      } finally {
        if (!cancelled) {
          setLoading(false)
        }
      }
    })()
    return () => {
      cancelled = true
    }
  }, [statusFilter, search, refreshKey])

  useEffect(() => {
    let cancelled = false
    void adminApi
      .getOverview()
      .then((data) => {
        if (!cancelled) {
          setOverview(data)
        }
      })
      .catch(() => undefined)
    return () => {
      cancelled = true
    }
  }, [refreshKey])

  const loadMore = async () => {
    if (!cursor || loadingMore) {
      return
    }
    setLoadingMore(true)
    try {
      const response = await adminApi.listUsers({
        status: statusFilter || undefined,
        q: search || undefined,
        cursor,
        limit: 50,
      })
      setUsers((current) => [...current, ...response.items])
      setCursor(response.next_cursor ?? null)
    } catch (caught) {
      showToast(errorMessage(caught), 'error')
    } finally {
      setLoadingMore(false)
    }
  }

  const pendingCount = overview?.status_counts.pending ?? 0
  const isDesktop = useMediaQuery('(min-width: 768px)')

  const renderUserRow = (user: AdminUserSummary) => (
    <div
      data-testid="admin-user-row"
      data-user-id={user.id}
      className="grid cursor-pointer gap-2 px-4 py-3 hover:bg-canvas md:grid-cols-[minmax(0,2.4fr)_100px_150px_150px_minmax(0,1.2fr)_150px] md:items-center md:gap-3"
      onClick={() => setSelectedId(user.id)}
    >
      <div className="min-w-0">
        <p className="truncate text-sm font-medium text-ink-900">
          {user.email}
        </p>
        <p className="mt-0.5 text-xs text-ink-500">
          {user.file_count} 个文件 · {user.folder_count} 个文件夹 ·{' '}
          {user.version_count} 个版本
          {user.is_admin ? ' · 管理员' : ''}
        </p>
      </div>
      <div className="flex items-center gap-2 md:block">
        <StatusBadge status={user.status} />
      </div>
      <p className="text-xs text-ink-600">{formatDateTime(user.created_at)}</p>
      <p className="text-xs text-ink-600">
        {formatDateTime(user.last_active_at)}
      </p>
      <div>
        <p className="text-xs text-ink-600">
          {formatBytes(user.used_bytes)} / {formatBytes(user.quota_bytes)}
        </p>
        <ProgressBar
          value={
            user.quota_bytes > 0
              ? (user.used_bytes / user.quota_bytes) * 100
              : 0
          }
          tone={user.used_bytes >= user.quota_bytes ? 'danger' : 'brand'}
          label="用户存储用量"
          heightClassName="mt-1.5 h-1.5"
        />
      </div>
      <div
        className="flex flex-wrap items-center gap-1.5 md:justify-end"
        onClick={(event) => event.stopPropagation()}
      >
        {user.status === 'pending' && (
          <>
            <Button
              size="sm"
              variant="primary"
              data-testid="admin-user-approve"
              onClick={() => setApproveTarget(user)}
            >
              批准
            </Button>
            <Button
              size="sm"
              variant="dangerGhost"
              data-testid="admin-user-reject"
              onClick={() => setDanger({ kind: 'reject', user })}
            >
              拒绝
            </Button>
          </>
        )}
        <button
          type="button"
          aria-label={`查看 ${user.email}`}
          className="flex h-8 items-center gap-1 rounded-control px-2 text-xs font-medium text-ink-600 hover:bg-canvas-strong hover:text-ink-900"
          onClick={() => setSelectedId(user.id)}
        >
          详情
          <ChevronRight size={15} aria-hidden="true" />
        </button>
      </div>
    </div>
  )

  if (isReauth(error)) {
    return <AdminReauthNotice />
  }

  return (
    <div data-testid="admin-users" className="space-y-5">
      <header className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 className="text-2xl font-semibold text-ink-900">用户</h1>
          <p className="mt-1 text-sm text-ink-600">
            审批账号、调整额度与状态。这里只显示账号元数据，没有文件名或内容。
          </p>
        </div>
      </header>

      <div className="flex flex-wrap items-center gap-2 border-b border-line pb-1">
        {TABS.map((tab) => {
          const active = statusFilter === tab.key
          return (
            <button
              key={tab.key || 'all'}
              type="button"
              data-testid={`admin-status-tab-${tab.key || 'all'}`}
              onClick={() => {
                if (tab.key) {
                  setSearchParams({ status: tab.key })
                } else {
                  setSearchParams({})
                }
              }}
              className={cx(
                '-mb-1 border-b-2 px-3 py-2 text-sm font-medium transition-colors motion-fast',
                active
                  ? 'border-brand-600 text-brand-600'
                  : 'border-transparent text-ink-600 hover:text-ink-900',
              )}
            >
              {tab.label}
              {tab.key === 'pending' && pendingCount > 0 && (
                <span className="ml-1.5 rounded-full bg-warning-50 px-1.5 py-0.5 text-xs text-warning-600">
                  {pendingCount}
                </span>
              )}
            </button>
          )
        })}
        <div className="ml-auto w-full sm:w-72">
          <TextField
            hideLabel
            label="按邮箱搜索"
            aria-label="按邮箱搜索用户"
            data-testid="admin-user-search"
            placeholder="按邮箱搜索"
            value={searchInput}
            onChange={(event) => setSearchInput(event.target.value)}
            leading={<Search size={16} />}
          />
        </div>
      </div>

      <AdminErrorBanner error={error} />

      <div className="overflow-hidden rounded-card border border-line">
        <div className="hidden grid-cols-[minmax(0,2.4fr)_100px_150px_150px_minmax(0,1.2fr)_150px] gap-3 border-b border-line bg-canvas px-4 py-2.5 text-xs font-medium text-ink-500 md:grid">
          <span>邮箱</span>
          <span>状态</span>
          <span>注册时间</span>
          <span>最近活动</span>
          <span>用量 / 额度</span>
          <span className="text-right">操作</span>
        </div>

        {loading ? (
          <div className="flex justify-center py-16">
            <Spinner size={26} label="加载用户" />
          </div>
        ) : users.length === 0 ? (
          <p className="px-4 py-16 text-center text-sm text-ink-600">
            没有符合条件的用户。
          </p>
        ) : isDesktop ? (
          <VirtualList
            items={users}
            getKey={(user) => user.id}
            renderItem={renderUserRow}
            height={560}
            rowHeight={76}
            testId="admin-user-virtual-list"
            onEndReached={
              cursor
                ? () => {
                    void loadMore()
                  }
                : undefined
            }
          />
        ) : (
          <ul>
            {users.map((user) => (
              <li key={user.id}>{renderUserRow(user)}</li>
            ))}
          </ul>
        )}
      </div>

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

      {approveTarget && (
        <ApproveDialog
          user={approveTarget}
          onClose={() => setApproveTarget(null)}
          onDone={() => {
            setApproveTarget(null)
            reload()
            showToast('已批准该账号', 'success')
          }}
        />
      )}

      {danger && (
        <DangerDialog
          kind={danger.kind}
          user={danger.user}
          onClose={() => setDanger(null)}
          onDone={() => {
            setDanger(null)
            reload()
          }}
        />
      )}

      {selectedId && (
        <UserDrawer
          userId={selectedId}
          onClose={() => setSelectedId(null)}
          onChanged={reload}
        />
      )}
    </div>
  )
}

function ApproveDialog({
  user,
  onClose,
  onDone,
}: {
  user: AdminUserSummary
  onClose: () => void
  onDone: () => void
}) {
  const [choice, setChoice] = useState('default')
  const [customGb, setCustomGb] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<unknown>(null)

  const quotaBytes = useMemo(() => {
    if (choice === 'default') {
      return undefined
    }
    if (choice === 'custom') {
      const value = Number(customGb)
      return Number.isFinite(value) && value >= 0 ? gbToBytes(value) : undefined
    }
    return gbToBytes(Number(choice))
  }, [choice, customGb])

  const submit = async () => {
    setBusy(true)
    setError(null)
    try {
      await adminApi.approve(user.id, quotaBytes)
      onDone()
    } catch (caught) {
      setError(caught)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog
      open
      onClose={onClose}
      title="批准账号"
      description={`为 ${user.email} 设定存储额度。`}
      testId="admin-approve-dialog"
      footer={
        <div className="flex justify-end gap-2">
          <Button variant="secondary" onClick={onClose} disabled={busy}>
            取消
          </Button>
          <Button
            variant="primary"
            data-testid="admin-approve-confirm"
            disabled={busy || (choice === 'custom' && quotaBytes === undefined)}
            onClick={() => void submit()}
          >
            {busy ? '批准中…' : '批准'}
          </Button>
        </div>
      }
    >
      <div className="space-y-3">
        <AdminErrorBanner error={error} />
        <fieldset className="space-y-2">
          <legend className="text-sm font-medium text-ink-900">额度</legend>
          <label className="flex items-center gap-2 text-sm text-ink-600">
            <input
              type="radio"
              name="approve-quota"
              value="default"
              checked={choice === 'default'}
              onChange={() => setChoice('default')}
            />
            使用默认额度
          </label>
          {QUOTA_PRESETS_GB.map((gb) => (
            <label
              key={gb}
              className="flex items-center gap-2 text-sm text-ink-600"
            >
              <input
                type="radio"
                name="approve-quota"
                value={String(gb)}
                checked={choice === String(gb)}
                onChange={() => setChoice(String(gb))}
              />
              {gb} GB
            </label>
          ))}
          <label className="flex items-center gap-2 text-sm text-ink-600">
            <input
              type="radio"
              name="approve-quota"
              value="custom"
              checked={choice === 'custom'}
              onChange={() => setChoice('custom')}
            />
            自定义
          </label>
        </fieldset>
        {choice === 'custom' && (
          <TextField
            label="自定义额度（GB）"
            type="number"
            min={0}
            step="0.5"
            value={customGb}
            onChange={(event) => setCustomGb(event.target.value)}
          />
        )}
      </div>
    </Dialog>
  )
}

function DangerDialog({
  kind,
  user,
  onClose,
  onDone,
}: {
  kind: 'reject' | 'suspend'
  user: AdminUserSummary
  onClose: () => void
  onDone: () => void
}) {
  const [reason, setReason] = useState('')
  const [confirmed, setConfirmed] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<unknown>(null)
  const { showToast } = useToast()
  const verb = kind === 'reject' ? '拒绝' : '停用'

  const submit = async () => {
    setBusy(true)
    setError(null)
    try {
      if (kind === 'reject') {
        await adminApi.reject(user.id, reason)
      } else {
        await adminApi.suspend(user.id, reason)
      }
      showToast(`已${verb}该账号`, 'success')
      onDone()
    } catch (caught) {
      setError(caught)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog
      open
      onClose={onClose}
      title={`${verb}账号`}
      description={
        kind === 'suspend'
          ? `停用 ${user.email} 会立即撤销其全部会话并中止进行中的上传。已有数据保留。`
          : `拒绝 ${user.email} 后该账号将无法登录。`
      }
      testId="admin-danger-dialog"
      footer={
        <div className="flex justify-end gap-2">
          <Button variant="secondary" onClick={onClose} disabled={busy}>
            取消
          </Button>
          <Button
            variant="danger"
            data-testid="admin-confirm-danger"
            disabled={busy || !confirmed || reason.trim().length === 0}
            onClick={() => void submit()}
          >
            {busy ? '提交中…' : `确认${verb}`}
          </Button>
        </div>
      }
    >
      <div className="space-y-3">
        <AdminErrorBanner error={error} />
        <label className="block text-sm font-medium text-ink-900">
          原因（会展示给用户，最多 500 字）
          <textarea
            data-testid="admin-reason"
            className="mt-1.5 min-h-24 w-full rounded-control border border-line bg-white px-3 py-2 text-sm text-ink-900 focus:border-brand-600 focus:outline-none focus:ring-2 focus:ring-brand-50"
            maxLength={500}
            value={reason}
            onChange={(event) => setReason(event.target.value)}
          />
        </label>
        <label className="flex items-start gap-2 text-sm text-ink-600">
          <input
            type="checkbox"
            className="mt-0.5"
            checked={confirmed}
            onChange={(event) => setConfirmed(event.target.checked)}
          />
          我确认要{verb}该账号
        </label>
      </div>
    </Dialog>
  )
}

function UserDrawer({
  userId,
  onClose,
  onChanged,
}: {
  userId: string
  onClose: () => void
  onChanged: () => void
}) {
  const [detail, setDetail] = useState<AdminUserDetailResponse | null>(null)
  const [error, setError] = useState<unknown>(null)
  const [loading, setLoading] = useState(true)
  const [quotaGb, setQuotaGb] = useState('')
  const [busy, setBusy] = useState(false)
  const [danger, setDanger] = useState<'reject' | 'suspend' | null>(null)
  const { showToast } = useToast()

  const reload = useCallback(async () => {
    setLoading(true)
    try {
      const response = await adminApi.getUser(userId)
      setDetail(response)
      setQuotaGb(String(bytesToGb(response.user.quota_bytes)))
      setError(null)
    } catch (caught) {
      setError(caught)
    } finally {
      setLoading(false)
    }
  }, [userId])

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const response = await adminApi.getUser(userId)
        if (!cancelled) {
          setDetail(response)
          setQuotaGb(String(bytesToGb(response.user.quota_bytes)))
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
  }, [userId])

  const run = async (action: () => Promise<unknown>, message: string) => {
    setBusy(true)
    try {
      await action()
      showToast(message, 'success')
      await reload()
      onChanged()
    } catch (caught) {
      showToast(errorMessage(caught), 'error')
    } finally {
      setBusy(false)
    }
  }

  if (isReauth(error)) {
    return <AdminReauthNotice />
  }

  return (
    <div className="fixed inset-0 z-40">
      <button
        type="button"
        aria-label="关闭用户详情"
        className="absolute inset-0 bg-ink-900/30 animate-overlay-in"
        onClick={onClose}
      />
      <aside
        data-testid="admin-detail-drawer"
        aria-label="用户详情"
        className="absolute inset-y-0 right-0 flex w-full max-w-xl flex-col overflow-y-auto border-l border-line bg-white shadow-pop animate-panel-in"
      >
        <header className="sticky top-0 flex items-start justify-between gap-3 border-b border-line bg-white px-5 py-4">
          <div className="min-w-0">
            <h2 className="truncate text-lg font-semibold text-ink-900">
              {detail?.user.email ?? '用户详情'}
            </h2>
            {detail && (
              <p className="mt-1 flex items-center gap-2 text-xs text-ink-600">
                <StatusBadge status={detail.user.status} />
                {detail.user.is_admin && <span>管理员</span>}
              </p>
            )}
          </div>
          <button
            type="button"
            aria-label="关闭"
            onClick={onClose}
            className="rounded-control p-1.5 text-ink-600 hover:bg-canvas"
          >
            <X size={20} />
          </button>
        </header>

        {loading && !detail ? (
          <div className="flex justify-center py-16">
            <Spinner size={26} label="加载用户详情" />
          </div>
        ) : !detail ? (
          <div className="p-5">
            <AdminErrorBanner error={error} />
          </div>
        ) : (
          <div className="space-y-6 p-5">
            <section className="grid grid-cols-2 gap-3 text-sm">
              <Info label="注册时间" value={formatDateTime(detail.user.created_at)} />
              <Info label="邮箱验证" value={formatDateTime(detail.user.email_verified_at)} />
              <Info label="批准时间" value={formatDateTime(detail.user.approved_at)} />
              <Info label="最近活动" value={formatDateTime(detail.user.last_active_at)} />
              <Info label="文件" value={String(detail.user.file_count)} />
              <Info label="文件夹" value={String(detail.user.folder_count)} />
              <Info label="版本" value={String(detail.user.version_count)} />
              <Info label="活动会话" value={String(detail.user.active_session_count)} />
            </section>

            <section className="rounded-control border border-line p-4">
              <p className="text-sm font-medium text-ink-900">用量与额度</p>
              <p className="mt-1 text-sm text-ink-600">
                已用 {formatBytes(detail.user.used_bytes)} · 预留{' '}
                {formatBytes(detail.user.reserved_bytes)} · 额度{' '}
                {formatBytes(detail.user.quota_bytes)}
              </p>
              <ProgressBar
                value={
                  detail.user.quota_bytes > 0
                    ? (detail.user.used_bytes / detail.user.quota_bytes) * 100
                    : 0
                }
                tone={
                  detail.user.used_bytes >= detail.user.quota_bytes
                    ? 'danger'
                    : 'brand'
                }
                label="用户存储用量"
                heightClassName="mt-2 h-2"
              />
              <div className="mt-4 flex items-end gap-2">
                <TextField
                  label="额度（GB）"
                  type="number"
                  min={0}
                  step="0.5"
                  value={quotaGb}
                  onChange={(event) => setQuotaGb(event.target.value)}
                  containerClassName="max-w-40"
                />
                <Button
                  data-testid="admin-save-quota"
                  disabled={busy}
                  onClick={() => {
                    const value = Number(quotaGb)
                    if (!Number.isFinite(value) || value < 0) {
                      showToast('请输入有效额度', 'error')
                      return
                    }
                    void run(
                      () =>
                        adminApi.updateQuota(
                          detail.user.id,
                          Math.round(value * GIB),
                        ),
                      '额度已更新',
                    )
                  }}
                >
                  保存额度
                </Button>
              </div>
            </section>

            <section className="flex flex-wrap gap-2">
              {detail.user.status === 'pending' && (
                <>
                  <Button
                    variant="primary"
                    size="sm"
                    data-testid="admin-detail-approve"
                    onClick={() => {
                      void run(
                        () => adminApi.approve(detail.user.id),
                        '已批准该账号',
                      )
                    }}
                  >
                    <Check size={15} aria-hidden="true" /> 批准
                  </Button>
                  <Button
                    variant="dangerGhost"
                    size="sm"
                    data-testid="admin-detail-reject"
                    onClick={() => setDanger('reject')}
                  >
                    <UserX size={15} aria-hidden="true" /> 拒绝
                  </Button>
                </>
              )}
              {(detail.user.status === 'active' ||
                detail.user.status === 'pending') && (
                <Button
                  variant="dangerGhost"
                  size="sm"
                  data-testid="admin-suspend"
                  onClick={() => setDanger('suspend')}
                >
                  <ShieldOff size={15} aria-hidden="true" /> 停用
                </Button>
              )}
              {(detail.user.status === 'suspended' ||
                detail.user.status === 'rejected') && (
                <Button
                  variant="primary"
                  size="sm"
                  data-testid="admin-reactivate"
                  onClick={() => {
                    void run(
                      () => adminApi.reactivate(detail.user.id),
                      '已恢复该账号',
                    )
                  }}
                >
                  <UserCheck size={15} aria-hidden="true" /> 恢复
                </Button>
              )}
              <Button
                variant="secondary"
                size="sm"
                data-testid="admin-revoke-sessions"
                disabled={busy}
                onClick={() => {
                  void run(
                    () => adminApi.revokeSessions(detail.user.id),
                    '已注销该用户的全部会话',
                  )
                }}
              >
                注销全部会话
              </Button>
              <Link
                to={`/admin/audit?target_user_id=${encodeURIComponent(detail.user.id)}`}
                className="inline-flex h-8 items-center rounded-control px-3 text-sm font-medium text-brand-600 hover:underline"
              >
                查看该用户审计
              </Link>
            </section>

            <section>
              <h3 className="text-sm font-semibold text-ink-900">
                最近会话（{detail.sessions.length}）
              </h3>
              <ul className="mt-2 divide-y divide-line rounded-control border border-line">
                {detail.sessions.length === 0 && (
                  <li className="px-3 py-3 text-sm text-ink-500">没有会话</li>
                )}
                {detail.sessions.map((session) => (
                  <li key={session.id} className="space-y-0.5 px-3 py-2.5 text-xs text-ink-600">
                    <p className="font-medium text-ink-900">
                      {session.ip}
                      {session.revoked ? ' · 已撤销' : ' · 活跃'}
                    </p>
                    <p>创建 {formatDateTime(session.created_at)}</p>
                    <p>最近 {formatDateTime(session.last_seen_at)}</p>
                    <p className="truncate">{session.user_agent ?? '未知设备'}</p>
                  </li>
                ))}
              </ul>
            </section>

            <section>
              <h3 className="text-sm font-semibold text-ink-900">
                最近审计（{detail.audit.length}）
              </h3>
              <ul className="mt-2 divide-y divide-line rounded-control border border-line">
                {detail.audit.length === 0 && (
                  <li className="px-3 py-3 text-sm text-ink-500">没有记录</li>
                )}
                {detail.audit.map((entry) => (
                  <li key={entry.id} className="space-y-1 px-3 py-2.5 text-xs text-ink-600">
                    <p className="font-medium text-ink-900">{entry.action}</p>
                    <p>{formatDateTime(entry.created_at)} · {entry.ip}</p>
                    <pre className="overflow-x-auto whitespace-pre-wrap break-all rounded bg-canvas p-2 text-[11px] leading-5 text-ink-600">
                      {JSON.stringify(entry.details, null, 2)}
                    </pre>
                  </li>
                ))}
              </ul>
            </section>
          </div>
        )}

        {danger && detail && (
          <DangerDialog
            kind={danger}
            user={detail.user}
            onClose={() => setDanger(null)}
            onDone={() => {
              setDanger(null)
              void reload()
              onChanged()
            }}
          />
        )}
      </aside>
    </div>
  )
}

function Info({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <p className="text-xs text-ink-500">{label}</p>
      <p className="mt-0.5 break-all text-sm text-ink-900">{value}</p>
    </div>
  )
}
