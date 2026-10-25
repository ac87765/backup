import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from 'react'
import { createPortal } from 'react-dom'
import { AlertCircle, CheckCircle2, Info } from 'lucide-react'

import { cx } from './cx'
import { ToastContext, type ToastTone } from './toast-context'

interface ToastItem {
  id: number
  message: string
  tone: ToastTone
}

const AUTO_DISMISS_MS = 3_000

const TONE_META: Record<
  ToastTone,
  { icon: ReactNode; className: string }
> = {
  success: {
    icon: <CheckCircle2 size={18} aria-hidden="true" />,
    className: 'text-success-600',
  },
  error: {
    icon: <AlertCircle size={18} aria-hidden="true" />,
    className: 'text-danger-600',
  },
  info: {
    icon: <Info size={18} aria-hidden="true" />,
    className: 'text-brand-600',
  },
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([])
  const nextId = useRef(0)
  const timers = useRef(new Map<number, ReturnType<typeof setTimeout>>())

  const dismiss = useCallback((id: number) => {
    const timer = timers.current.get(id)
    if (timer) {
      clearTimeout(timer)
      timers.current.delete(id)
    }
    setToasts((previous) => previous.filter((toast) => toast.id !== id))
  }, [])

  const showToast = useCallback(
    (message: string, tone: ToastTone = 'success') => {
      const id = nextId.current
      nextId.current += 1
      setToasts((previous) => [...previous, { id, message, tone }])
      const timer = setTimeout(() => dismiss(id), AUTO_DISMISS_MS)
      timers.current.set(id, timer)
    },
    [dismiss],
  )

  useEffect(() => {
    const pending = timers.current
    return () => {
      for (const timer of pending.values()) {
        clearTimeout(timer)
      }
      pending.clear()
    }
  }, [])

  return (
    <ToastContext.Provider value={{ showToast }}>
      {children}
      {toasts.length > 0 &&
        createPortal(
          <div
            role="status"
            aria-live="polite"
            className="pointer-events-none fixed right-4 top-4 z-[60] flex w-[calc(100%-2rem)] max-w-sm flex-col gap-2"
          >
            {toasts.map((toast) => (
              <div
                key={toast.id}
                data-testid="toast"
                data-tone={toast.tone}
                className={cx(
                  'pointer-events-auto flex items-start gap-2.5 rounded-card border border-line bg-white px-4 py-3 text-sm leading-6 text-ink-900 shadow-pop',
                  TONE_META[toast.tone].className,
                )}
              >
                <span className="mt-0.5 shrink-0">
                  {TONE_META[toast.tone].icon}
                </span>
                <span className="min-w-0 flex-1 break-words text-ink-900">
                  {toast.message}
                </span>
              </div>
            ))}
          </div>,
          document.body,
        )}
    </ToastContext.Provider>
  )
}
