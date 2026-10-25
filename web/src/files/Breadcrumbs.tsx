import { ChevronRight } from 'lucide-react'

import { cx } from '../ui'

export interface Breadcrumb {
  id: string
  name: string
}

export function Breadcrumbs({
  path,
  onNavigate,
}: {
  path: Breadcrumb[]
  onNavigate: (index: number) => void
}) {
  return (
    <nav aria-label="面包屑" className="flex min-w-0 items-center gap-0.5 text-sm">
      {path.map((entry, index) => {
        const current = index === path.length - 1
        return (
          <span key={entry.id} className="flex min-w-0 items-center gap-0.5">
            {index > 0 && (
              <ChevronRight
                size={14}
                className="shrink-0 text-ink-400"
                aria-hidden="true"
              />
            )}
            {current ? (
              <span
                aria-current="page"
                className="truncate px-1.5 py-1 font-medium text-ink-900"
              >
                {entry.name}
              </span>
            ) : (
              <button
                type="button"
                onClick={() => onNavigate(index)}
                className={cx(
                  'max-w-[12rem] truncate rounded-control px-1.5 py-1 text-ink-600 transition-colors hover:bg-canvas hover:text-ink-900',
                  'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600',
                )}
              >
                {entry.name}
              </button>
            )}
          </span>
        )
      })}
    </nav>
  )
}
