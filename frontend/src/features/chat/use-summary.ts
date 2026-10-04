import { useCallback, useEffect, useState } from 'react'
import { api, isApiError, type ApiError } from '@/lib/api'
import type { SummaryView } from '@/lib/types'

/** 輪詢間隔與上限（毫秒）；頁面可用 props 覆寫，測試因此不必等真實時間。 */
const SUMMARY_POLL_MS = 2000
const SUMMARY_MAX_POLL_MS = 90_000

type SummaryState =
  | { kind: 'pending' }
  | { kind: 'ready'; summary: SummaryView }
  | { kind: 'failed' }
  /** 輪詢超過上限，總結仍在產生中 */
  | { kind: 'timeout' }
  | { kind: 'error'; error: ApiError | undefined }

type Options = { pollMs?: number; maxPollMs?: number }

/**
 * 輪詢 `GET /api/conversations/{id}/summary`：pending 時每隔 `pollMs` 再查一次，
 * ready／failed 停止；超過 `maxPollMs` 顯示 timeout。離開頁面（unmount）時中止請求與計時器。
 */
export function useSummary(conversationId: string, { pollMs = SUMMARY_POLL_MS, maxPollMs = SUMMARY_MAX_POLL_MS }: Options = {}) {
  const [state, setState] = useState<SummaryState>({ kind: 'pending' })
  const [round, setRound] = useState(0)
  const [retrying, setRetrying] = useState(false)
  const [retryError, setRetryError] = useState<unknown>()

  useEffect(() => {
    const ctrl = new AbortController()
    let timer: ReturnType<typeof setTimeout> | undefined
    const startedAt = Date.now()

    const poll = async () => {
      try {
        const s = await api.get<SummaryView>(`/api/conversations/${conversationId}/summary`, ctrl.signal)
        if (ctrl.signal.aborted) return
        if (s.status === 'ready') setState({ kind: 'ready', summary: s })
        else if (s.status === 'failed') setState({ kind: 'failed' })
        else if (Date.now() - startedAt >= maxPollMs) setState({ kind: 'timeout' })
        else {
          setState({ kind: 'pending' })
          timer = setTimeout(poll, pollMs)
        }
      } catch (e) {
        if (ctrl.signal.aborted || (e instanceof DOMException && e.name === 'AbortError')) return
        setState({ kind: 'error', error: isApiError(e) ? e : undefined })
      }
    }
    void poll()

    return () => {
      ctrl.abort()
      clearTimeout(timer)
    }
  }, [conversationId, round, pollMs, maxPollMs])

  /** 重新開始輪詢（逾時或查詢失敗後的「再檢查一次」）。 */
  const check = useCallback(() => {
    setState({ kind: 'pending' })
    setRound((n) => n + 1)
  }, [])

  /** 總結失敗後重新產生（POST summary/retry），成功後回到輪詢。 */
  const retry = useCallback(async () => {
    setRetrying(true)
    setRetryError(undefined)
    try {
      await api.post(`/api/conversations/${conversationId}/summary/retry`)
      check()
    } catch (e) {
      setRetryError(e)
    } finally {
      setRetrying(false)
    }
  }, [conversationId, check])

  return { state, check, retry, retrying, retryError }
}
