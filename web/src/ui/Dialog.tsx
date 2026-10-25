import {
  useEffect,
  useId,
  useRef,
  type ReactNode,
  type RefObject,
} from 'react'
import { createPortal } from 'react-dom'
import { X } from 'lucide-react'

import { cx } from './cx'
import { IconButton } from './IconButton'

export type DialogSize = 'sm' | 'md' | 'lg'

const SIZES: Record<DialogSize, string> = {
  sm: 'max-w-sm',
  md: 'max-w-lg',
  lg: 'max-w-2xl',
}

export interface DialogProps {
  open: boolean
  onClose: () => void
  title: ReactNode
  description?: ReactNode
  children?: ReactNode
  footer?: ReactNode
  size?: DialogSize
  /** 打开后优先聚焦的元素；未提供时聚焦 `[data-dialog-autofocus]` 或首个可聚焦元素。 */
  initialFocusRef?: RefObject<HTMLElement | null>
  testId?: string
}

const FOCUSABLE =
  'input:not([type="hidden"]):not([disabled]), select:not([disabled]), textarea:not([disabled]), button:not([disabled]), [href], [tabindex]:not([tabindex="-1"])'

/**
 * 基于原生 `<dialog>` 的模态框：showModal() 提供焦点陷阱与顶层渲染，
 * 同时自行处理 Esc、遮罩点击、初始焦点与关闭后的焦点归还
 * （jsdom 等缺少 showModal 的环境退化为 open 属性）。
 */
export function Dialog({
  open,
  onClose,
  title,
  description,
  children,
  footer,
  size = 'md',
  initialFocusRef,
  testId,
}: DialogProps) {
  const dialogRef = useRef<HTMLDialogElement>(null)
  const previousFocusRef = useRef<HTMLElement | null>(null)
  const titleId = useId()
  const descriptionId = useId()

  // 打开 / 关闭：原生弹窗、初始焦点、关闭时归还焦点。
  useEffect(() => {
    const dialog = dialogRef.current
    if (!dialog || !open) {
      return
    }
    previousFocusRef.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null

    if (typeof dialog.showModal === 'function') {
      if (!dialog.open) {
        dialog.showModal()
      }
    } else {
      dialog.setAttribute('open', '')
    }

    const target =
      initialFocusRef?.current ??
      dialog.querySelector<HTMLElement>('[data-dialog-autofocus]') ??
      dialog.querySelector<HTMLElement>(FOCUSABLE)
    target?.focus()

    const previous = previousFocusRef.current
    return () => {
      if (typeof dialog.close === 'function') {
        if (dialog.open) {
          dialog.close()
        }
      } else {
        dialog.removeAttribute('open')
      }
      if (previous?.isConnected) {
        previous.focus()
      }
    }
  }, [initialFocusRef, open])

  // Esc：统一走 onClose，避免原生 cancel 与自定义监听重复触发。
  useEffect(() => {
    if (!open) {
      return
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault()
        event.stopPropagation()
        onClose()
      }
    }
    document.addEventListener('keydown', onKeyDown, true)
    return () => document.removeEventListener('keydown', onKeyDown, true)
  }, [onClose, open])

  if (!open) {
    return null
  }

  return createPortal(
    <dialog
      ref={dialogRef}
      data-testid={testId}
      aria-modal="true"
      aria-labelledby={titleId}
      aria-describedby={description ? descriptionId : undefined}
      className={cx(
        'fixed inset-0 z-50 m-auto w-[calc(100%-2rem)] overflow-hidden rounded-card border border-line bg-white p-0 text-ink-900 shadow-pop backdrop:bg-ink-900/40',
        SIZES[size],
      )}
      onCancel={(event) => {
        event.preventDefault()
        onClose()
      }}
      onClick={(event) => {
        if (event.target === event.currentTarget) {
          onClose()
        }
      }}
    >
      <div className="flex items-start justify-between gap-4 border-b border-line px-6 py-4">
        <div className="min-w-0 space-y-1">
          <h2 id={titleId} className="text-base font-semibold text-ink-900">
            {title}
          </h2>
          {description && (
            <p id={descriptionId} className="text-sm leading-6 text-ink-600">
              {description}
            </p>
          )}
        </div>
        <IconButton label="关闭" size="sm" onClick={onClose}>
          <X size={18} />
        </IconButton>
      </div>
      {children && <div className="px-6 py-5">{children}</div>}
      {footer && (
        <div className="flex flex-wrap justify-end gap-2 border-t border-line bg-canvas px-6 py-4">
          {footer}
        </div>
      )}
    </dialog>,
    document.body,
  )
}
