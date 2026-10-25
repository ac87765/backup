import { useState, type ReactNode } from 'react'

import { cx } from '../ui/cx'

interface VirtualListProps<T> {
  items: T[]
  getKey: (item: T) => string
  renderItem: (item: T) => ReactNode
  height?: number
  rowHeight?: number
  testId?: string
  empty?: ReactNode
  onEndReached?: () => void
  className?: string
  rowClassName?: string
}

/**
 * 最小虚拟滚动列表：只渲染视口附近的行，支持任意长度目录。
 * 行高固定，滚动位置用 `onScroll` 跟踪；不引入额外依赖。
 */
export function VirtualList<T>({
  items,
  getKey,
  renderItem,
  height = 420,
  rowHeight = 56,
  testId = 'virtual-list',
  empty,
  onEndReached,
  className,
  rowClassName = 'border-b border-line',
}: VirtualListProps<T>) {
  const [scrollTop, setScrollTop] = useState(0)
  if (items.length === 0) {
    return (
      <div data-testid={testId} className={className}>
        {empty ?? null}
      </div>
    )
  }

  const overscan = 4
  const first = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan)
  const visibleCount = Math.ceil(height / rowHeight) + overscan * 2
  const last = Math.min(items.length, first + visibleCount)
  const visible = items.slice(first, last)

  return (
    <div
      data-testid={testId}
      className={cx('relative overflow-y-auto', className)}
      style={{ height }}
      onScroll={(event) => {
        const target = event.currentTarget
        setScrollTop(target.scrollTop)
        if (
          onEndReached &&
          target.scrollTop + target.clientHeight >= target.scrollHeight - rowHeight
        ) {
          onEndReached()
        }
      }}
    >
      <div style={{ height: items.length * rowHeight, position: 'relative' }}>
        {visible.map((item, index) => {
          const position = first + index
          return (
            <div
              key={getKey(item)}
              className={cx('absolute inset-x-0', rowClassName)}
              style={{ top: position * rowHeight, height: rowHeight }}
            >
              {renderItem(item)}
            </div>
          )
        })}
      </div>
    </div>
  )
}
