import { useEffect, useRef, useState, type ReactNode } from 'react'

import { cx, useMediaQuery } from '../ui'

export interface RevealProps {
  children: ReactNode
  className?: string
}

/**
 * 区块进入视口时淡入上移（design D1 / D2）。
 *
 * IntersectionObserver 只负责切换一个 class；不支持该 API 或系统要求减少
 * 动态效果时直接显示，不留下隐藏内容。
 */
export function Reveal({ children, className }: RevealProps) {
  const reducedMotion = useMediaQuery('(prefers-reduced-motion: reduce)')
  const ref = useRef<HTMLDivElement>(null)
  const [revealed, setRevealed] = useState(false)
  const visible =
    revealed || reducedMotion || typeof IntersectionObserver === 'undefined'

  useEffect(() => {
    if (visible) {
      return
    }
    const node = ref.current
    if (!node) {
      return
    }

    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          setRevealed(true)
          observer.disconnect()
        }
      },
      { rootMargin: '0px 0px -8% 0px' },
    )
    observer.observe(node)
    return () => observer.disconnect()
  }, [visible])

  return (
    <div
      ref={ref}
      className={cx(
        className,
        !reducedMotion && 'motion-reveal',
        visible ? 'translate-y-0 opacity-100' : 'translate-y-2 opacity-0',
      )}
    >
      {children}
    </div>
  )
}
