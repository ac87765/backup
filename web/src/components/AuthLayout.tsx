import type { ReactNode } from 'react'

import { Brand } from '../shell/Brand'
import { cx } from '../ui'

export interface AuthLayoutProps {
  title: string
  subtitle?: ReactNode
  children: ReactNode
  footer?: ReactNode
  size?: 'md' | 'lg'
}

/** 登录、注册、验证、解锁、重置与未登录状态的安全说明页共用品牌布局。 */
export function AuthLayout({
  title,
  subtitle,
  children,
  footer,
  size = 'md',
}: AuthLayoutProps) {
  return (
    <main className="flex min-h-dvh flex-col items-center justify-center bg-canvas px-4 py-10">
      <div
        className={cx(
          'w-full space-y-6',
          size === 'lg' ? 'max-w-3xl' : 'max-w-md',
        )}
      >
        <header className="flex flex-col items-center gap-3 text-center">
          <Brand />
          <div className="space-y-1.5">
            <h1 className="text-2xl font-semibold tracking-tight text-ink-900">
              {title}
            </h1>
            {subtitle && (
              <p className="mx-auto max-w-lg text-sm leading-6 text-ink-600">
                {subtitle}
              </p>
            )}
          </div>
        </header>
        <section className="rounded-card border border-line bg-white p-6 sm:p-8">
          {children}
        </section>
        {footer && (
          <footer className="text-center text-sm leading-6 text-ink-600">
            {footer}
          </footer>
        )}
      </div>
    </main>
  )
}
