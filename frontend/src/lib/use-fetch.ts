import { useCallback, useEffect, useRef, useState } from 'react'
import { api, isAbortError, isApiError, type ApiError } from '@/lib/api'
import type { Page } from '@/lib/types'

export type FetchState<T> = {
  data: T | undefined
  error: ApiError | undefined
  loading: boolean
  /** 重新載入（保留舊資料直到新資料回來） */
  reload: () => void
}

/** GET 一個 JSON 端點；`path` 為 `null` 時不發請求。 */
export function useFetch<T>(path: string | null): FetchState<T> {
  const [data, setData] = useState<T>()
  const [error, setError] = useState<ApiError>()
  const [loading, setLoading] = useState(path !== null)
  const [tick, setTick] = useState(0)

  useEffect(() => {
    if (path === null) return
    const ctrl = new AbortController()
    setLoading(true)
    api
      .get<T>(path, ctrl.signal)
      .then((d) => {
        setData(d)
        setError(undefined)
      })
      .catch((e: unknown) => {
        if (isAbortError(e)) return
        setError(isApiError(e) ? e : undefined)
      })
      .finally(() => {
        if (!ctrl.signal.aborted) setLoading(false)
      })
    return () => ctrl.abort()
  }, [path, tick])

  const reload = useCallback(() => setTick((n) => n + 1), [])
  return { data, error, loading, reload }
}

export type PagedState<T, P extends Page<T>> = {
  /** 已載入的所有項目（依序串接各頁） */
  items: T[] | undefined
  /** 最後載入的一頁；列表另帶的欄位（例如學生列表的 `total`）從這裡取 */
  page: P | undefined
  /** 第一頁載入或重新載入失敗 */
  error: ApiError | undefined
  /** 載入更多失敗；已載入的項目照舊顯示 */
  moreError: ApiError | undefined
  /** 第一頁載入中 */
  loading: boolean
  loadingMore: boolean
  hasMore: boolean
  loadMore: () => void
  /** 從第一頁重新載入（保留舊資料直到新資料回來） */
  reload: () => void
}

/**
 * GET 一個 keyset 分頁列表：先載入第一頁，`loadMore` 再依 `next_cursor` 載入下一頁並接在後面。
 * `path` 可帶查詢字串（篩選、搜尋）；`path` 改變時從第一頁重新載入。`path` 為 `null` 時不發請求。
 */
export function usePagedFetch<T, P extends Page<T> = Page<T>>(path: string | null): PagedState<T, P> {
  const [loaded, setLoaded] = useState<{ path: string | null; pages: P[] }>()
  const [error, setError] = useState<ApiError>()
  const [moreError, setMoreError] = useState<ApiError>()
  const [loading, setLoading] = useState(path !== null)
  const [loadingMore, setLoadingMore] = useState(false)
  const [tick, setTick] = useState(0)
  const ctrl = useRef<AbortController>(undefined)

  useEffect(() => {
    if (path === null) return
    const c = new AbortController()
    ctrl.current = c
    setLoading(true)
    setLoadingMore(false)
    setMoreError(undefined)
    api
      .get<P>(path, c.signal)
      .then((first) => {
        setLoaded({ path, pages: [first] })
        setError(undefined)
      })
      .catch((e: unknown) => {
        if (isAbortError(e)) return
        setError(isApiError(e) ? e : undefined)
      })
      .finally(() => {
        if (!c.signal.aborted) setLoading(false)
      })
    return () => c.abort()
  }, [path, tick])

  // 換了 path（篩選、檢視）後，舊路徑的資料不再顯示，直到新的第一頁回來
  const pages = loaded?.path === path ? loaded.pages : undefined
  const last = pages?.[pages.length - 1]
  const cursor = last?.next_cursor ?? null

  const loadMore = useCallback(() => {
    const c = ctrl.current
    if (path === null || !c || c.signal.aborted || cursor === null || loadingMore) return
    setLoadingMore(true)
    setMoreError(undefined)
    const sep = path.includes('?') ? '&' : '?'
    api
      .get<P>(`${path}${sep}cursor=${encodeURIComponent(cursor)}`, c.signal)
      .then((next) =>
        setLoaded((previous) =>
          previous?.path === path ? { path, pages: [...previous.pages, next] } : previous,
        ),
      )
      .catch((e: unknown) => {
        if (isAbortError(e)) return
        setMoreError(isApiError(e) ? e : undefined)
      })
      .finally(() => {
        if (!c.signal.aborted) setLoadingMore(false)
      })
  }, [path, cursor, loadingMore])

  const reload = useCallback(() => setTick((n) => n + 1), [])
  return {
    items: pages?.flatMap((p) => p.items),
    page: last,
    error,
    moreError,
    loading,
    loadingMore,
    hasMore: cursor !== null,
    loadMore,
    reload,
  }
}
