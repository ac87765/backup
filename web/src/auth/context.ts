import { createContext, useContext } from 'react'

import type { CurrentUser, WorkerApi } from '../worker/types'

export interface AuthContextValue {
  api: WorkerApi | null
  user: CurrentUser | null
  loading: boolean
  unlocked: boolean
  setSession: (user: CurrentUser, unlocked: boolean) => void
  markUnlocked: () => void
  logout: () => Promise<void>
}

export const AuthContext = createContext<AuthContextValue | null>(null)

export function useAuth(): AuthContextValue {
  const value = useContext(AuthContext)
  if (!value) {
    throw new Error('useAuth 必须在 AuthProvider 内使用')
  }
  return value
}
