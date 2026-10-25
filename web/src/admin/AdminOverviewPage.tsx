import { useEffect, useState, type ReactNode } from 'react'
import { useNavigate } from 'react-router-dom'
import {
  Activity,
  CheckCircle2,
  Clock3,
  Database,
  Mail,
  Server,
  Trash2,
  Users,
} from 'lucide-react'

import { formatBytes } from '../files/format'
import { Button, ProgressBar, Spinner } from '../ui'
import { AdminApiError, adminApi, type AdminOverview } from './api'
import { AdminErrorBanner, AdminReauthNotice } from './AdminLayout'

function StatCard({
  label,
  value,
  hint,
  tone = 'default',
  onClick,
  testId,
}: {
  label: string
  value: string | number
  hint?: string
  tone?: 'default' | 'warning'
  onClick?: () => void
  testId?: string
}) {
  const content = (
    <>
      <p className="text-sm font-medium text-ink-600">{label}</p>
      <p
        className={
          tone === 'warning'
            ? 'mt-2 text-3xl font-semibold text-warning-600'
            : 'mt-2 text-3xl font-semibold text-ink-900'
        }
      >
        {value}
      </p>
      {hint && <p className="mt-1 text-xs text-ink-500">{hint}</p>}
    </>
  )
  if (onClick) {
    return (
      <button
        type="button"
        data-testid={testId}
        onClick={onClick}
        className="rounded-card border border-line bg-white p-5 text-left transition-colors motion-fast hover:border-line-strong hover:bg-canvas focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600"
      >
        {content}
      </button>
    )
  }
  return (
    <section
      data-testid={testId}
      className="rounded-card border border-line bg-white p-5"
    >
      {content}
    </section>
  )
}

export function AdminOverviewPage() {
  const navigate = useNavigate()
  const [data, setData] = useState<AdminOverview | null>(null)
  const [error, setError] = useState<unknown>(null)
  const [loading, setLoading] = useState(true)

  useEffect(() => {
    let cancelled = false
    void (async () => {
      try {
        const overview = await adminApi.getOverview()
        if (!cancelled) {
          setData(overview)
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
  }, [])

  if (error instanceof AdminApiError && error.code === 'reauth_required') {
    return <AdminReauthNotice />
  }
  if (loading) {
    return (
      <div className="flex justify-center py-20">
        <Spinner size={28} label="加载概览" />
      </div>
    )
  }
  if (!data) {
    return <AdminErrorBanner error={error} />
  }

  const usagePercent =
    data.total_quota_bytes > 0
      ? (data.total_used_bytes / data.total_quota_bytes) * 100
      : 0

  return (
    <div data-testid="admin-overview" className="space-y-6">
      <header>
        <h1 className="text-2xl font-semibold text-ink-900">概览</h1>
        <p className="mt-1 text-sm text-ink-600">
          账号状态、存储用量、队列积压与服务健康。
        </p>
      </header>

      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <StatCard
          label="待审核"
          value={data.pending_users}
          hint="点击查看待审核用户"
          tone="warning"
          testId="admin-pending-count"
          onClick={() => void navigate('/admin/users?status=pending')}
        />
        <StatCard label="正常" value={data.status_counts.active} />
        <StatCard label="已停用" value={data.status_counts.suspended} />
        <StatCard label="已拒绝" value={data.status_counts.rejected} />
      </div>

      <section className="rounded-card border border-line bg-white p-5">
        <div className="flex flex-wrap items-end justify-between gap-2">
          <div>
            <h2 className="text-base font-semibold text-ink-900">总用量</h2>
            <p className="mt-1 text-sm text-ink-600">
              已用 {formatBytes(data.total_used_bytes)} / 总额度{' '}
              {formatBytes(data.total_quota_bytes)}
            </p>
          </div>
          <p className="text-xs text-ink-500">
            预留 {formatBytes(data.total_reserved_bytes)}
          </p>
        </div>
        <ProgressBar
          value={usagePercent}
          tone={usagePercent >= 90 ? 'danger' : 'brand'}
          label="总存储用量"
          heightClassName="mt-4 h-2.5"
        />
      </section>

      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <section className="rounded-card border border-line bg-white p-5">
          <p className="flex items-center gap-2 text-sm font-medium text-ink-600">
            <Users size={16} aria-hidden="true" /> 近 7 天注册
          </p>
          <p className="mt-2 text-2xl font-semibold text-ink-900">
            {data.registrations_last_7_days}
          </p>
        </section>
        <section className="rounded-card border border-line bg-white p-5">
          <p className="flex items-center gap-2 text-sm font-medium text-ink-600">
            <Mail size={16} aria-hidden="true" /> 发信队列
          </p>
          <p className="mt-2 text-2xl font-semibold text-ink-900">
            {data.queues.email_outbox}
          </p>
        </section>
        <section className="rounded-card border border-line bg-white p-5">
          <p className="flex items-center gap-2 text-sm font-medium text-ink-600">
            <Trash2 size={16} aria-hidden="true" /> 对象删除队列
          </p>
          <p className="mt-2 text-2xl font-semibold text-ink-900">
            {data.queues.object_deletions}
          </p>
        </section>
        <section className="rounded-card border border-line bg-white p-5">
          <p className="flex items-center gap-2 text-sm font-medium text-ink-600">
            <Server size={16} aria-hidden="true" /> 服务版本
          </p>
          <p className="mt-2 text-2xl font-semibold text-ink-900">
            {data.version}
          </p>
        </section>
      </div>

      <section className="rounded-card border border-line bg-white p-5">
        <h2 className="flex items-center gap-2 text-base font-semibold text-ink-900">
          <Activity size={17} aria-hidden="true" /> 服务健康
        </h2>
        <div className="mt-4 grid gap-4 sm:grid-cols-3">
          <HealthItem label="总体" value={data.health.status} />
          <HealthItem label="数据库" value={data.health.database} icon={<Database size={15} />} />
          <HealthItem label="对象存储" value={data.health.storage} icon={<Clock3 size={15} />} />
        </div>
      </section>

      <Button
        variant="secondary"
        onClick={() => void navigate('/admin/users?status=pending')}
      >
        查看待审核用户
      </Button>
    </div>
  )
}

function HealthItem({
  label,
  value,
  icon,
}: {
  label: string
  value: string
  icon?: ReactNode
}) {
  const healthy = value === 'ready' || value === 'ok'
  return (
    <div className="rounded-control border border-line px-4 py-3">
      <p className="flex items-center gap-1.5 text-xs text-ink-500">
        {icon}
        {label}
      </p>
      <p
        className={
          healthy
            ? 'mt-1 flex items-center gap-1.5 text-sm font-medium text-success-700'
            : 'mt-1 flex items-center gap-1.5 text-sm font-medium text-danger-600'
        }
      >
        <CheckCircle2 size={15} aria-hidden="true" />
        {value}
      </p>
    </div>
  )
}
