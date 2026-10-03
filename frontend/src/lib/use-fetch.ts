import { useCallback, useEffect, useState } from 'react'
import { api, isApiError, type ApiError } from '@/lib/api'

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
        if (e instanceof DOMException && e.name === 'AbortError') return
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
