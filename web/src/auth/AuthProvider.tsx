import { useCallback, useEffect, useMemo, useState, type ReactNode } from 'react'

import { getCryptoWorker } from '../worker/client'
import type { CurrentUser } from '../worker/types'
import { AuthContext, type AuthContextValue } from './context'

export function AuthProvider({ children }: { children: ReactNode }) {
  const api = useMemo(() => {
    try {
      return getCryptoWorker()
    } catch {
      // jsdom 等没有 Worker 的环境：页面仍可渲染，认证操作会报错。
      return null
    }
  }, [])
  const [user, setUser] = useState<CurrentUser | null>(null)
  const [loading, setLoading] = useState(api !== null)
  const [unlocked, setUnlocked] = useState(false)

  const setSession = useCallback((nextUser: CurrentUser, nextUnlocked: boolean) => {
    setUser(nextUser)
    setUnlocked(nextUnlocked)
  }, [])

  const markUnlocked = useCallback(() => {
    setUnlocked(true)
  }, [])

  const logout = useCallback(async () => {
    if (api) {
      try {
        await api.logout()
      } finally {
        setUser(null)
        setUnlocked(false)
      }
    }
  }, [api])

  useEffect(() => {
    if (!api) {
      return
    }
    let cancelled = false
    void (async () => {
      try {
        const current = await api.refreshSession()
        if (cancelled) {
          return
        }
        if (current) {
          setUser(current)
          setUnlocked(!(await api.isLocked()))
        } else {
          setUser(null)
          setUnlocked(false)
        }
      } catch {
        if (!cancelled) {
          setUser(null)
          setUnlocked(false)
        }
      } finally {
        if (!cancelled) {
          setLoading(false)
        }
      }
    })()
    return () => {
      cancelled = true
    }
  }, [api])

  const value: AuthContextValue = {
    api,
    user,
    loading,
    unlocked,
    setSession,
    markUnlocked,
    logout,
  }

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}
