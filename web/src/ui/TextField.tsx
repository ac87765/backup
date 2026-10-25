import { useId, type InputHTMLAttributes, type ReactNode } from 'react'

import { cx } from './cx'

export interface TextFieldProps
  extends Omit<InputHTMLAttributes<HTMLInputElement>, 'size'> {
  label?: ReactNode
  /** 视觉上隐藏 label，但仍保留给读屏。 */
  hideLabel?: boolean
  error?: string | null
  hint?: ReactNode
  leading?: ReactNode
  containerClassName?: string
  inputClassName?: string
}

export function TextField({
  label,
  hideLabel = false,
  error,
  hint,
  leading,
  className,
  containerClassName,
  inputClassName,
  id,
  ...rest
}: TextFieldProps) {
  const generatedId = useId()
  const inputId = id ?? `field-${generatedId}`
  const hintId = hint ? `${inputId}-hint` : undefined
  const errorId = error ? `${inputId}-error` : undefined
  const describedBy = [hintId, errorId].filter(Boolean).join(' ') || undefined

  return (
    <div className={cx('w-full', containerClassName, className)}>
      {label && (
        <label
          htmlFor={inputId}
          className={cx(
            'mb-1.5 block text-sm font-medium text-ink-900',
            hideLabel && 'sr-only',
          )}
        >
          {label}
        </label>
      )}
      <div className="relative">
        {leading && (
          <span
            aria-hidden="true"
            className="pointer-events-none absolute left-3 top-1/2 flex -translate-y-1/2 text-ink-400"
          >
            {leading}
          </span>
        )}
        <input
          id={inputId}
          aria-invalid={error ? true : undefined}
          data-invalid={error ? 'true' : undefined}
          aria-describedby={describedBy}
          className={cx(
            'h-10 w-full rounded-control border border-line bg-white pr-3 text-sm text-ink-900 transition-colors placeholder:text-ink-500 focus:border-brand-600 focus:outline-none focus:ring-2 focus:ring-brand-50 disabled:cursor-not-allowed disabled:bg-canvas disabled:text-ink-600',
            'data-[invalid=true]:border-danger-600 data-[invalid=true]:focus:border-danger-600 data-[invalid=true]:focus:ring-danger-50',
            leading ? 'pl-9' : 'pl-3',
            inputClassName,
          )}
          {...rest}
        />
      </div>
      {hint && (
        <p id={hintId} className="mt-1.5 text-xs leading-5 text-ink-600">
          {hint}
        </p>
      )}
      {error && (
        <p id={errorId} role="alert" className="mt-1.5 text-xs leading-5 text-danger-600">
          {error}
        </p>
      )}
    </div>
  )
}
