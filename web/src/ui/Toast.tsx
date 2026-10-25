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
  closing: boolean
}

interface AutoTimer {
  timeout: ReturnType<typeof setTimeout> | null
  deadline: number
  remaining: number
}

const AUTO_DISMISS_MS = 3_000
const TOAST_EXIT_MS = 180
const MAX_VISIBLE = 3

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
  const itemsRef = useRef<ToastItem[]>([])
  const nextId = useRef(0)
  const autoTimers = useRef(new Map<number, AutoTimer>())
  const removeTimers = useRef(
    new Map<number, ReturnType<typeof setTimeout>>(),
  )

  const commit = useCallback((next: ToastItem[]) => {
    itemsRef.current = next
    setToasts(next)
  }, [])

  const clearAuto = useCallback((id: number) => {
    const timer = autoTimers.current.get(id)
    if (timer?.timeout) {
      clearTimeout(timer.timeout)
    }
    autoTimers.current.delete(id)
  }, [])

  const clearRemove = useCallback((id: number) => {
    const timer = removeTimers.current.get(id)
    if (timer) {
      clearTimeout(timer)
      removeTimers.current.delete(id)
    }
  }, [])

  const remove = useCallback(
    (id: number) => {
      clearAuto(id)
      clearRemove(id)
      commit(itemsRef.current.filter((toast) => toast.id !== id))
    },
    [clearAuto, clearRemove, commit],
  )

  const beginClose = useCallback(
    (id: number) => {
      const current = itemsRef.current.find((toast) => toast.id === id)
      if (!current || current.closing) {
        return
      }
      clearAuto(id)
      commit(
        itemsRef.current.map((toast) =>
          toast.id === id ? { ...toast, closing: true } : toast,
        ),
      )
      const timer = setTimeout(() => remove(id), TOAST_EXIT_MS)
      removeTimers.current.set(id, timer)
    },
    [clearAuto, commit, remove],
  )

  const scheduleClose = useCallback(
    (id: number, delay: number) => {
      clearAuto(id)
      const timeout = setTimeout(() => {
        autoTimers.current.delete(id)
        beginClose(id)
      }, delay)
      autoTimers.current.set(id, {
        timeout,
        deadline: Date.now() + delay,
        remaining: delay,
      })
    },
    [beginClose, clearAuto],
  )

  const pause = useCallback((id: number) => {
    const timer = autoTimers.current.get(id)
    if (!timer?.timeout) {
      return
    }
    clearTimeout(timer.timeout)
    autoTimers.current.set(id, {
      timeout: null,
      deadline: 0,
      remaining: Math.max(0, timer.deadline - Date.now()),
    })
  }, [])

  const resume = useCallback(
    (id: number) => {
      const timer = autoTimers.current.get(id)
      if (!timer || timer.timeout) {
        return
      }
      scheduleClose(id, timer.remaining || AUTO_DISMISS_MS)
    },
    [scheduleClose],
  )

  const showToast = useCallback(
    (message: string, tone: ToastTone = 'success') => {
      const existing = itemsRef.current.find(
        (toast) => toast.message === message,
      )
      if (existing) {
        clearRemove(existing.id)
        commit(
          itemsRef.current.map((toast) =>
            toast.id === existing.id
              ? { ...toast, tone, closing: false }
              : toast,
          ),
        )
        scheduleClose(existing.id, AUTO_DISMISS_MS)
        return
      }

      let next = [...itemsRef.current]
      if (next.length >= MAX_VISIBLE) {
        const oldest = next[0]
        clearAuto(oldest.id)
        clearRemove(oldest.id)
        next = next.slice(1)
      }
      const id = nextId.current
      nextId.current += 1
      next.push({ id, message, tone, closing: false })
      commit(next)
      scheduleClose(id, AUTO_DISMISS_MS)
    },
    [clearAuto, clearRemove, commit, scheduleClose],
  )

  useEffect(() => {
    const autos = autoTimers.current
    const removes = removeTimers.current
    return () => {
      for (const timer of autos.values()) {
        if (timer.timeout) {
          clearTimeout(timer.timeout)
        }
      }
      for (const timer of removes.values()) {
        clearTimeout(timer)
      }
      autos.clear()
      removes.clear()
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
            className="pointer-events-none fixed bottom-4 left-4 z-[60] flex w-[calc(100%-2rem)] max-w-sm flex-col gap-2 md:left-64"
          >
            {toasts.map((toast) => (
              <div
                key={toast.id}
                data-testid="toast"
                data-tone={toast.tone}
                data-closing={toast.closing ? 'true' : 'false'}
                onPointerEnter={() => pause(toast.id)}
                onPointerLeave={() => resume(toast.id)}
                className={cx(
                  'flex items-start gap-2.5 rounded-card border border-line bg-white px-4 py-3 text-sm leading-6 text-ink-900 shadow-pop',
                  toast.closing
                    ? 'animate-toast-out pointer-events-none'
                    : 'animate-toast-in pointer-events-auto',
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
