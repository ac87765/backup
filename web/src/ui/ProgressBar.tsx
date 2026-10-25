import { cx } from './cx'

export type ProgressTone = 'brand' | 'danger' | 'success'

const TONES: Record<ProgressTone, string> = {
  brand: 'bg-brand-600',
  danger: 'bg-danger-600',
  success: 'bg-success-600',
}

export interface ProgressBarProps {
  /** 0–100；超出范围会被截断。 */
  value: number
  tone?: ProgressTone
  label?: string
  testId?: string
  className?: string
  /** 高度工具类，默认 1.5（6px）。 */
  heightClassName?: string
}

export function ProgressBar({
  value,
  tone = 'brand',
  label,
  testId,
  className,
  heightClassName = 'h-1.5',
}: ProgressBarProps) {
  const clamped = Math.min(100, Math.max(0, Number.isFinite(value) ? value : 0))
  return (
    <div
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(clamped)}
      aria-label={label}
      data-testid={testId}
      className={cx(
        'w-full overflow-hidden rounded-full bg-canvas-strong',
        heightClassName,
        className,
      )}
    >
      <div
        className={cx(
          'h-full rounded-full transition-[width] motion-base ease-linear',
          TONES[tone],
        )}
        style={{ width: `${clamped}%` }}
      />
    </div>
  )
}
