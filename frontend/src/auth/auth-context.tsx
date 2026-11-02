import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from 'react'
import { api, isApiError } from '@/lib/api'
import { useI18n } from '@/i18n'
import { isLang, type Lang } from '@/i18n/define'
import type { Me, Roles } from '@/lib/types'

const LOGIN_PICK_KEY = 'lang-picked-before-login'
/** 登入前選的語言最多保留多久：夠走完一次 Google 登入的來回，又不會留給下一個人。 */
const LOGIN_PICK_TTL_MS = 10 * 60_000

type LoginPick = { lang: Lang; at: number }

function readLoginPick(): LoginPick | null {
  try {
    const raw = sessionStorage.getItem(LOGIN_PICK_KEY)
    const pick = raw ? (JSON.parse(raw) as Partial<LoginPick>) : null
    if (!pick || typeof pick.at !== 'number' || !isLang(pick.lang)) return null
    return { lang: pick.lang, at: pick.at }
  } catch {
    return null
  }
}

/** 登入前（登入頁）使用者明確選的語言；登入完成後要以它為準，不能被伺服器的初始值蓋掉。 */
export function rememberLoginPick(lang: Lang) {
  try {
    sessionStorage.setItem(LOGIN_PICK_KEY, JSON.stringify({ lang, at: Date.now() }))
  } catch {
    // sessionStorage 不可用（隱私模式等）：退回以帳號語言為準
  }
}

function clearLoginPick() {
  try {
    sessionStorage.removeItem(LOGIN_PICK_KEY)
  } catch {
    // 忽略
  }
}

/** 只清掉在 `before` 之前留下的選擇（之後才選的還在等登入結果）。 */
function clearStaleLoginPick(before: number) {
  const current = readLoginPick()
  if (!current || current.at < before) clearLoginPick()
}

/** 取出並清掉登入前的選擇；過期的當作沒有。 */
function takeLoginPick(): Lang | null {
  const pick = readLoginPick()
  clearLoginPick()
  return pick && Date.now() - pick.at <= LOGIN_PICK_TTL_MS ? pick.lang : null
}

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
  /** 用後端剛回傳的使用者直接更新（例如改名後），不必再打一次 /api/me */
  update: (me: Me) => void
  logout: () => Promise<void>
}

const AuthContext = createContext<AuthValue | null>(null)

export function AuthProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<AuthState>({ status: 'loading' })
  const { setLang } = useI18n()

  const refresh = useCallback(async () => {
    const started = Date.now()
    try {
      let me = await api.get<Me>('/api/me')
      const pick = takeLoginPick()
      if (pick && pick !== me.language) {
        // 帳號第一次登入時語言由伺服器依 Accept-Language 初始化；使用者在登入頁的選擇優先
        try {
          me = await api.patch<Me>('/api/me', { language: pick })
        } catch {
          // 存不進去就維持帳號原本的語言
        }
      }
      setLang(me.language)
      setState({ status: 'authed', me })
    } catch (e) {
      // 401 = 沒登入；其他（網路、500）顯示錯誤而不是假裝沒登入
      if (isApiError(e) && e.status === 401) {
        // 沒有人登入：這次載入之前留下的選擇不屬於任何人，別留給下一個登入的人
        clearStaleLoginPick(started)
        setState({ status: 'anon' })
      }
      else if (isApiError(e) && e.code === 'account_disabled') setState({ status: 'disabled' })
      else setState({ status: 'error' })
    }
  }, [setLang])

  useEffect(() => {
    void refresh()
  }, [refresh])

  const logout = useCallback(async () => {
    try {
      await api.post('/api/auth/logout')
    } finally {
      clearLoginPick()
      setState({ status: 'anon' })
    }
  }, [])

  const update = useCallback((me: Me) => setState({ status: 'authed', me }), [])

  const value = useMemo<AuthValue>(() => ({ ...state, refresh, update, logout }), [state, refresh, update, logout])
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
