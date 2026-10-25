import {
  cloneElement,
  Fragment,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type ReactElement,
  type ReactNode,
  type RefObject,
} from 'react'
import { createPortal } from 'react-dom'

import { cx } from './cx'

export interface MenuItem {
  id?: string
  label: string
  icon?: ReactNode
  onSelect: () => void
  tone?: 'default' | 'danger'
  disabled?: boolean
  /** 可访问名称；文件操作要求 `动作 {文件名}`（D8）。 */
  ariaLabel?: string
  testId?: string
  separatorBefore?: boolean
}

interface Point {
  top: number
  left: number
}

const MENU_WIDTH = 208

function clampPoint(left: number, top: number): Point {
  // 预留约 280px 的菜单高度（最多 6 项 + 分隔线 + 账号信息），避免贴近底部时溢出。
  return {
    top: Math.max(8, Math.min(top, window.innerHeight - 280)),
    left: Math.max(8, Math.min(left, window.innerWidth - MENU_WIDTH - 8)),
  }
}

interface MenuSurfaceProps {
  id: string
  label: string
  items: MenuItem[]
  point: Point
  /** 菜单顶部的非交互内容（如账号信息）。 */
  header?: ReactNode
  /** 点击这些元素内部不算“点击外部”。 */
  ignoreRef?: RefObject<HTMLElement | null>
  onClose: () => void
}

function MenuSurface({
  id,
  label,
  items,
  point,
  header,
  ignoreRef,
  onClose,
}: MenuSurfaceProps) {
  const menuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const first = menuRef.current?.querySelector<HTMLButtonElement>(
      '[role="menuitem"]:not([disabled])',
    )
    first?.focus()
  }, [])

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node | null
      if (!target) {
        return
      }
      if (menuRef.current?.contains(target)) {
        return
      }
      if (ignoreRef?.current?.contains(target)) {
        return
      }
      onClose()
    }
    document.addEventListener('pointerdown', onPointerDown, true)
    return () =>
      document.removeEventListener('pointerdown', onPointerDown, true)
  }, [ignoreRef, onClose])

  const onKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const menuItems = Array.from(
      menuRef.current?.querySelectorAll<HTMLButtonElement>(
        '[role="menuitem"]:not([disabled])',
      ) ?? [],
    )
    if (menuItems.length === 0) {
      return
    }
    const active = document.activeElement
    const index = menuItems.findIndex((item) => item === active)
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      menuItems[(index + 1 + menuItems.length) % menuItems.length]?.focus()
    } else if (event.key === 'ArrowUp') {
      event.preventDefault()
      menuItems[
        (index - 1 + menuItems.length) % menuItems.length
      ]?.focus()
    } else if (event.key === 'Home') {
      event.preventDefault()
      menuItems[0]?.focus()
    } else if (event.key === 'End') {
      event.preventDefault()
      menuItems[menuItems.length - 1]?.focus()
    } else if (event.key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      onClose()
    } else if (event.key === 'Tab') {
      onClose()
    }
  }

  return createPortal(
    <div
      ref={menuRef}
      id={id}
      role="menu"
      aria-label={label}
      data-testid="menu"
      onKeyDown={onKeyDown}
      style={{
        position: 'fixed',
        top: point.top,
        left: point.left,
        width: MENU_WIDTH,
      }}
      className="z-50 overflow-hidden rounded-card border border-line bg-white p-1.5 shadow-pop"
    >
      {header && <div className="border-b border-line px-2.5 pb-2.5 pt-1.5">{header}</div>}
      {items.map((item) => (
        <Fragment key={item.id ?? item.label}>
          {item.separatorBefore && (
            <div role="separator" className="my-1 h-px bg-line" />
          )}
          <button
            type="button"
            role="menuitem"
            tabIndex={-1}
            disabled={item.disabled}
            aria-label={item.ariaLabel}
            data-testid={item.testId}
            onClick={() => {
              if (item.disabled) {
                return
              }
              onClose()
              item.onSelect()
            }}
            className={cx(
              'flex w-full items-center gap-2.5 rounded-control px-2.5 py-2 text-left text-sm transition-colors',
              item.tone === 'danger'
                ? 'text-danger-700 hover:bg-danger-50'
                : 'text-ink-900 hover:bg-canvas',
              'focus-visible:bg-canvas focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand-600',
              'disabled:cursor-not-allowed disabled:opacity-40',
            )}
          >
            {item.icon && (
              <span aria-hidden="true" className="flex shrink-0 text-ink-600">
                {item.icon}
              </span>
            )}
            <span className="truncate">{item.label}</span>
          </button>
        </Fragment>
      ))}
    </div>,
    document.body,
  )
}

export interface MenuProps {
  /** 菜单的可访问名称，例如“更多操作 合同.pdf”。 */
  label: string
  items: MenuItem[]
  trigger: ReactElement
  align?: 'start' | 'end'
  disabled?: boolean
  /** 菜单顶部的非交互内容（如账号信息）。 */
  header?: ReactNode
}

/** 下拉菜单：Enter/Space/ArrowDown 打开，方向键导航，Esc / 外部点击关闭。 */
export function Menu({
  label,
  items,
  trigger,
  align = 'end',
  disabled = false,
  header,
}: MenuProps) {
  const [point, setPoint] = useState<Point | null>(null)
  const triggerRef = useRef<HTMLButtonElement>(null)
  const menuId = useId()

  const close = useCallback(() => {
    setPoint(null)
    triggerRef.current?.focus()
  }, [])

  const openMenu = useCallback(() => {
    const element = triggerRef.current
    if (!element) {
      return
    }
    const rect = element.getBoundingClientRect()
    setPoint(
      clampPoint(
        align === 'end' ? rect.right - MENU_WIDTH : rect.left,
        rect.bottom + 8,
      ),
    )
  }, [align])

  const triggerProps = {
    ref: triggerRef,
    'aria-haspopup': 'menu' as const,
    'aria-expanded': point !== null,
    'aria-controls': point ? menuId : undefined,
    disabled,
    onClick: (event: ReactMouseEvent<HTMLButtonElement>) => {
      event.preventDefault()
      if (point) {
        setPoint(null)
      } else {
        openMenu()
      }
    },
    onKeyDown: (event: ReactKeyboardEvent<HTMLButtonElement>) => {
      if (
        event.key === 'Enter' ||
        event.key === ' ' ||
        event.key === 'ArrowDown'
      ) {
        event.preventDefault()
        if (!point) {
          openMenu()
        }
      } else if (event.key === 'Escape' && point) {
        event.preventDefault()
        close()
      }
    },
  }

  return (
    <>
      {cloneElement(
        trigger as ReactElement<Record<string, unknown>>,
        triggerProps,
      )}
      {point && (
        <MenuSurface
          id={menuId}
          label={label}
          items={items}
          point={point}
          header={header}
          ignoreRef={triggerRef}
          onClose={close}
        />
      )}
    </>
  )
}

export interface ContextMenuProps {
  label: string
  items: MenuItem[]
  /** 指针位置；为 null 时不渲染。 */
  point: { x: number; y: number } | null
  onClose: () => void
  /** 关闭后归还焦点的元素（通常是触发行）。 */
  restoreFocusTo?: HTMLElement | null
}

/** 右键菜单：与下拉菜单共用同一套菜单项和键盘行为。 */
export function ContextMenu({
  label,
  items,
  point,
  onClose,
  restoreFocusTo,
}: ContextMenuProps) {
  const menuId = useId()
  const close = useCallback(() => {
    onClose()
    if (restoreFocusTo?.isConnected) {
      restoreFocusTo.focus()
    }
  }, [onClose, restoreFocusTo])

  if (!point) {
    return null
  }

  return (
    <MenuSurface
      id={menuId}
      label={label}
      items={items}
      point={clampPoint(point.x, point.y)}
      onClose={close}
    />
  )
}
