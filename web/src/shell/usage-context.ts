import { createContext, useContext } from 'react'

import type { UsageView } from '../worker/types'

export interface UsageContextValue {
  usage: UsageView | null
  /** 上传/删除/恢复后刷新侧边栏用量。 */
  refreshUsage: () => void
}

export const UsageContext = createContext<UsageContextValue | null>(null)

export function useUsage(): UsageContextValue {
  const value = useContext(UsageContext)
  if (!value) {
    throw new Error('useUsage 必须在 UsageProvider 内使用')
  }
  return value
}
