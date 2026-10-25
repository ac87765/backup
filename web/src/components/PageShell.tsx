import type { ReactNode } from 'react'

export function PageShell({
  title,
  subtitle,
  children,
}: {
  title: string
  subtitle?: string
  children: ReactNode
}) {
  return (
    <main className="mx-auto flex min-h-dvh w-full max-w-xl flex-col justify-center gap-6 p-6">
      <header className="space-y-2">
        <h1 className="text-3xl font-semibold tracking-tight text-slate-900">
          {title}
        </h1>
        {subtitle && <p className="text-slate-600">{subtitle}</p>}
      </header>
      <section className="rounded-2xl border border-slate-200 bg-white p-6 shadow-sm">
        {children}
      </section>
    </main>
  )
}

export function FormError({ message }: { message: string | null }) {
  if (!message) {
    return null
  }
  return (
    <p role="alert" className="rounded-lg bg-red-50 p-3 text-sm text-red-700">
      {message}
    </p>
  )
}

export function FormSuccess({ message }: { message: string | null }) {
  if (!message) {
    return null
  }
  return (
    <p role="status" className="rounded-lg bg-emerald-50 p-3 text-sm text-emerald-800">
      {message}
    </p>
  )
}
