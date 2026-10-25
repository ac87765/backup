import type { ButtonHTMLAttributes, ReactNode } from 'react'

import { cx } from './cx'

export type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger'
export type ButtonSize = 'sm' | 'md'

const VARIANTS: Record<ButtonVariant, string> = {
  primary: 'bg-brand-600 text-white hover:bg-brand-700 active:bg-brand-700',
  secondary:
    'border border-line bg-white text-ink-900 hover:border-line-strong hover:bg-canvas active:bg-canvas-strong',
  ghost: 'text-ink-600 hover:bg-canvas hover:text-ink-900 active:bg-canvas-strong',
  danger: 'bg-danger-600 text-white hover:bg-danger-700 active:bg-danger-700',
}

const SIZES: Record<ButtonSize, string> = {
  sm: 'h-8 gap-1.5 px-3 text-sm',
  md: 'h-10 gap-2 px-4 text-sm',
}

const BASE =
  'inline-flex select-none items-center justify-center whitespace-nowrap rounded-control font-medium transition-colors duration-150 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600 focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50'

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant
  size?: ButtonSize
  /** 按钮文字前的图标（纯装饰，自动 aria-hidden）。 */
  icon?: ReactNode
  block?: boolean
}

export function Button({
  variant = 'secondary',
  size = 'md',
  icon,
  block = false,
  className,
  children,
  type = 'button',
  ...rest
}: ButtonProps) {
  return (
    <button
      type={type}
      className={cx(
        BASE,
        VARIANTS[variant],
        SIZES[size],
        block && 'w-full',
        className,
      )}
      {...rest}
    >
      {icon && (
        <span aria-hidden="true" className="flex shrink-0">
          {icon}
        </span>
      )}
      {children}
    </button>
  )
}
