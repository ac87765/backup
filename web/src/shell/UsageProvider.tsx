import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react'

import { useAuth } from '../auth/context'
import type { UsageView } from '../worker/types'
import { UsageContext } from './usage-context'

/** 外壳级用量状态：侧边栏展示，文件/回收站操作后可刷新。 */
export function UsageProvider({ children }: { children: ReactNode }) {
  const { api } = useAuth()
  const [usage, setUsage] = useState<UsageView | null>(null)
  const [reloadToken, setReloadToken] = useState(0)

  const refreshUsage = useCallback(() => {
    setReloadToken((value) => value + 1)
  }, [])

  useEffect(() => {
    if (!api) {
      return
    }
    let cancelled = false
    void (async () => {
      try {
        const next = await api.getUsage()
        if (!cancelled) {
          setUsage(next)
        }
      } catch {
        // 用量失败不阻塞文件浏览；侧边栏显示占位符。
      }
    })()
    return () => {
      cancelled = true
    }
  }, [api, reloadToken])

  const value = useMemo(
    () => ({ usage, refreshUsage }),
    [refreshUsage, usage],
  )

  return <UsageContext.Provider value={value}>{children}</UsageContext.Provider>
}
