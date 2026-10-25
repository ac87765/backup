import { Loader2 } from 'lucide-react'

import { cx } from './cx'

export function Spinner({
  className,
  size = 16,
  label,
}: {
  className?: string
  size?: number
  /** 传入时作为读屏文本；不传则视为纯装饰。 */
  label?: string
}) {
  return (
    <span
      className={cx('inline-flex items-center justify-center', className)}
      role={label ? 'status' : undefined}
      aria-label={label}
    >
      <Loader2
        aria-hidden="true"
        size={size}
        className="animate-spin"
        focusable="false"
      />
    </span>
  )
}
