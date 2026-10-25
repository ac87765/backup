import { Fragment, type ReactNode } from 'react'
import { ChevronRight } from 'lucide-react'

export interface PageHeaderAncestor {
  id: string
  label: string
  onClick?: () => void
}

export interface PageHeaderProps {
  ancestors?: PageHeaderAncestor[]
  title: string
  subtitle?: ReactNode
  actions?: ReactNode
}

/**
 * 面包屑即标题：祖先路径在上方作为小号灰色链接，当前项是 H1。
 * data-testid="breadcrumb" 保留在整个标题区，兼容旧 E2E 定位。
 */
export function PageHeader({
  ancestors = [],
  title,
  subtitle,
  actions,
}: PageHeaderProps) {
  return (
    <header
      data-testid="breadcrumb"
      className="flex flex-wrap items-start justify-between gap-3"
    >
      <div className="min-w-0 space-y-1">
        {ancestors.length > 0 && (
          <nav
            aria-label="面包屑"
            className="flex min-w-0 flex-wrap items-center gap-0.5 text-xs text-ink-500"
          >
            {ancestors.map((ancestor, index) => (
              <Fragment key={ancestor.id}>
                {index > 0 && (
                  <ChevronRight
                    size={12}
                    aria-hidden="true"
                    className="shrink-0 text-ink-400"
                  />
                )}
                {ancestor.onClick ? (
                  <button
                    type="button"
                    onClick={ancestor.onClick}
                    className="max-w-[12rem] truncate rounded-control px-1 py-0.5 transition-colors hover:bg-canvas hover:text-ink-900 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600 motion-fast"
                  >
                    {ancestor.label}
                  </button>
                ) : (
                  <span className="truncate px-1 py-0.5">{ancestor.label}</span>
                )}
              </Fragment>
            ))}
          </nav>
        )}
        <h1
          title={title}
          className="sr-only text-2xl font-semibold tracking-tight text-ink-900 md:not-sr-only md:truncate"
        >
          {title}
        </h1>
        {subtitle && (
          <div className="text-sm leading-6 text-ink-600">{subtitle}</div>
        )}
      </div>
      {actions && (
        <div className="flex shrink-0 items-center gap-2">{actions}</div>
      )}
    </header>
  )
}
