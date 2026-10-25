import { Link } from 'react-router-dom'
import { ShieldCheck } from 'lucide-react'

import { cx } from '../ui'

export function Brand({
  to,
  size = 'md',
}: {
  to?: string
  size?: 'sm' | 'md'
}) {
  const mark = (
    <>
      <span
        className={cx(
          'flex items-center justify-center rounded-control bg-brand-600 text-white',
          size === 'sm' ? 'h-8 w-8' : 'h-9 w-9',
        )}
      >
        <ShieldCheck
          size={size === 'sm' ? 18 : 20}
          aria-hidden="true"
        />
      </span>
      <span
        className={cx(
          'font-semibold tracking-tight text-ink-900',
          size === 'sm' ? 'text-base' : 'text-[17px]',
        )}
      >
        加密备份
      </span>
    </>
  )

  if (to) {
    return (
      <Link
        to={to}
        aria-label="加密备份 首页"
        className="flex items-center gap-2.5 rounded-control focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600"
      >
        {mark}
      </Link>
    )
  }

  return <div className="flex items-center gap-2.5">{mark}</div>
}
