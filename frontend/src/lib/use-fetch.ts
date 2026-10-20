import { useCallback, useEffect, useState } from 'react'
import { api, isApiError, type ApiError } from '@/lib/api'
import type { Page } from '@/lib/types'

export type FetchState<T> = {
  data: T | undefined
  error: ApiError | undefined
  loading: boolean
  /** 重新載入（保留舊資料直到新資料回來） */
  reload: () => void
}

type Loader = (path: string, signal: AbortSignal) => Promise<unknown>

const getOne: Loader = (path, signal) => api.get(path, signal)

/** 依序跟著 `next_cursor` 取完分頁列表（每頁 200 筆，上限）。 */
const getAll: Loader = async (path, signal) => {
  const items: unknown[] = []
  let cursor: string | null = null
  do {
    const query: string = cursor ? `?limit=200&cursor=${encodeURIComponent(cursor)}` : '?limit=200'
    const page: Page<unknown> = await api.get<Page<unknown>>(`${path}${query}`, signal)
    items.push(...page.items)
    cursor = page.next_cursor
  } while (cursor)
  return items
}

function useLoad<T>(path: string | null, load: Loader): FetchState<T> {
  const [data, setData] = useState<T>()
  const [error, setError] = useState<ApiError>()
  const [loading, setLoading] = useState(path !== null)
  const [tick, setTick] = useState(0)

  useEffect(() => {
    if (path === null) return
    const ctrl = new AbortController()
    setLoading(true)
    load(path, ctrl.signal)
      .then((d) => {
        setData(d as T)
        setError(undefined)
      })
      .catch((e: unknown) => {
        if (e instanceof DOMException && e.name === 'AbortError') return
        setError(isApiError(e) ? e : undefined)
      })
      .finally(() => {
        if (!ctrl.signal.aborted) setLoading(false)
      })
    return () => ctrl.abort()
  }, [path, load, tick])

  const reload = useCallback(() => setTick((n) => n + 1), [])
  return { data, error, loading, reload }
}

/** GET 一個 JSON 端點；`path` 為 `null` 時不發請求。 */
export function useFetch<T>(path: string | null): FetchState<T> {
  return useLoad<T>(path, getOne)
}

/** 取回分頁列表的全部項目（畫面需要完整清單才能計數或搜尋時用）；`path` 不含查詢字串。 */
export function useFetchAll<T>(path: string | null): FetchState<T[]> {
  return useLoad<T[]>(path, getAll)
}
