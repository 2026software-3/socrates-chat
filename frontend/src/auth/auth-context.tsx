import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { api, isApiError } from '@/lib/api'
import type { Me, Roles } from '@/lib/types'

type AuthState =
  | { status: 'loading' }
  | { status: 'anon' }
  | { status: 'error' }
  /** 帳號已被管理者停用（S-01.3） */
  | { status: 'disabled' }
  | { status: 'authed'; me: Me }

type AuthValue = AuthState & {
  /** 重新讀取目前登入者（例如名單異動後） */
  refresh: () => Promise<void>
  logout: () => Promise<void>
}

const AuthContext = createContext<AuthValue | null>(null)

export function AuthProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<AuthState>({ status: 'loading' })

  const refresh = useCallback(async () => {
    try {
      const me = await api.get<Me>('/api/me')
      setState({ status: 'authed', me })
    } catch (e) {
      // 401 = 沒登入；其他（網路、500）顯示錯誤而不是假裝沒登入
      if (isApiError(e) && e.status === 401) setState({ status: 'anon' })
      else if (isApiError(e) && e.code === 'account_disabled') setState({ status: 'disabled' })
      else setState({ status: 'error' })
    }
  }, [])

  useEffect(() => {
    void refresh()
  }, [refresh])

  const logout = useCallback(async () => {
    try {
      await api.post('/api/auth/logout')
    } finally {
      setState({ status: 'anon' })
    }
  }, [])

  const value = useMemo<AuthValue>(() => ({ ...state, refresh, logout }), [state, refresh, logout])
  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth(): AuthValue {
  const ctx = useContext(AuthContext)
  if (!ctx) throw new Error('useAuth must be used inside <AuthProvider>')
  return ctx
}

/** 已登入頁面專用：取得目前使用者（路由守門保證一定有）。 */
export function useMe(): Me {
  const auth = useAuth()
  if (auth.status !== 'authed') throw new Error('useMe requires an authenticated user')
  return auth.me
}

/** 至少有一種角色（尚未開通的帳號三種都沒有）。 */
export function hasAnyRole(r: Roles): boolean {
  return r.admin || r.teacher || r.student
}
