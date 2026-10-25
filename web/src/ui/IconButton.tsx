import type { ButtonHTMLAttributes, ReactNode, Ref } from 'react'

import { cx } from './cx'

export type IconButtonVariant = 'ghost' | 'secondary' | 'danger'
export type IconButtonSize = 'sm' | 'md'

const VARIANTS: Record<IconButtonVariant, string> = {
  ghost: 'text-ink-600 hover:bg-canvas hover:text-ink-900',
  secondary:
    'border border-line bg-white text-ink-600 hover:border-line-strong hover:bg-canvas hover:text-ink-900',
  danger: 'text-danger-600 hover:bg-danger-50 hover:text-danger-700',
}

const SIZES: Record<IconButtonSize, string> = {
  sm: 'h-8 w-8',
  md: 'h-9 w-9',
}

export interface IconButtonProps
  extends ButtonHTMLAttributes<HTMLButtonElement> {
  /** 必填：图标按钮必须有可访问名称（D8 / a11y）。 */
  label: string
  variant?: IconButtonVariant
  size?: IconButtonSize
  children: ReactNode
  ref?: Ref<HTMLButtonElement>
}

export function IconButton({
  label,
  variant = 'ghost',
  size = 'md',
  className,
  children,
  type = 'button',
  ...rest
}: IconButtonProps) {
  return (
    <button
      type={type}
      aria-label={label}
      title={label}
      className={cx(
        'inline-flex shrink-0 items-center justify-center rounded-control transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600 focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-40',
        VARIANTS[variant],
        SIZES[size],
        className,
      )}
      {...rest}
    >
      <span aria-hidden="true" className="flex">
        {children}
      </span>
    </button>
  )
}
