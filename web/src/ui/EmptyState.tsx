import type { ReactNode } from 'react'

import { cx } from './cx'

export interface EmptyStateProps {
  icon?: ReactNode
  title: ReactNode
  description?: ReactNode
  /** 主/次操作按钮。 */
  actions?: ReactNode
  className?: string
}

export function EmptyState({
  icon,
  title,
  description,
  actions,
  className,
}: EmptyStateProps) {
  return (
    <div
      className={cx(
        'flex flex-col items-center gap-3 px-6 py-16 text-center',
        className,
      )}
    >
      {icon && (
        <span
          aria-hidden="true"
          className="flex h-14 w-14 items-center justify-center rounded-full bg-brand-50 text-brand-600"
        >
          {icon}
        </span>
      )}
      <p className="text-base font-semibold text-ink-900">{title}</p>
      {description && (
        <p className="max-w-sm text-sm leading-6 text-ink-600">{description}</p>
      )}
      {actions && (
        <div className="mt-2 flex flex-wrap items-center justify-center gap-2">
          {actions}
        </div>
      )}
    </div>
  )
}
