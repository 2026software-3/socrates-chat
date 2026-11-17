import { useCallback, useEffect, useRef, useState } from 'react'
import { api, isAbortError, isApiError, type ApiError } from '@/lib/api'
import type { RadarView } from '@/lib/types'

/** 輪詢間隔與上限（毫秒）；頁面可用 props 覆寫，測試因此不必等真實時間。 */
const RADAR_POLL_MS = 2000
const RADAR_MAX_POLL_MS = 90_000

export type RadarState =
  /** `initial`：第一次讀取，還不知道有沒有東西可顯示 */
  | { kind: 'loading'; initial: boolean }
  | { kind: 'loaded'; radar: RadarView }
  /** 輪詢超過上限，仍在計算中 */
  | { kind: 'timeout' }
  | { kind: 'error'; error: ApiError | undefined }

type Options = { pollMs?: number; maxPollMs?: number }

/**
 * 讀取 `GET /api/conversations/{id}/radar`。狀態與能否重試都以後端回應為準：
 * 只有「計算中且還不能重試」時每隔 `pollMs` 再查一次，超過 `maxPollMs` 顯示 timeout；
 * 其他狀態都停下來顯示。離開頁面（unmount）時中止請求與計時器。
 */
export function useRadar(conversationId: string, { pollMs = RADAR_POLL_MS, maxPollMs = RADAR_MAX_POLL_MS }: Options = {}) {
  const [state, setState] = useState<RadarState>({ kind: 'loading', initial: true })
  const [round, setRound] = useState(0)
  const [starting, setStarting] = useState(false)
  const [startError, setStartError] = useState<unknown>()
  // 按鈕停用要等下一次渲染才生效；用 ref 擋掉同一輪事件裡的重複送出
  const busy = useRef(false)

  useEffect(() => {
    const ctrl = new AbortController()
    let timer: ReturnType<typeof setTimeout> | undefined
    const startedAt = Date.now()

    const poll = async () => {
      try {
        const radar = await api.get<RadarView>(`/api/conversations/${conversationId}/radar`, ctrl.signal)
        if (ctrl.signal.aborted) return
        const computing = radar.status === 'pending' && !radar.retryable
        if (computing && Date.now() - startedAt >= maxPollMs) setState({ kind: 'timeout' })
        else {
          setState({ kind: 'loaded', radar })
          if (computing) timer = setTimeout(poll, pollMs)
        }
      } catch (e) {
        if (ctrl.signal.aborted || isAbortError(e)) return
        setState({ kind: 'error', error: isApiError(e) ? e : undefined })
      }
    }
    void poll()

    return () => {
      ctrl.abort()
      clearTimeout(timer)
    }
  }, [conversationId, round, pollMs, maxPollMs])

  /** 重新讀取（逾時、讀取失敗或等待題目維度時的「再檢查一次」）。 */
  const check = useCallback(() => {
    setStartError(undefined)
    setState({ kind: 'loading', initial: false })
    setRound((n) => n + 1)
  }, [])

  /** 開始或重新計算（POST radar/retry）；成功後回到輪詢，失敗時保留目前狀態並回報錯誤。 */
  const start = useCallback(async () => {
    if (busy.current) return
    busy.current = true
    setStarting(true)
    setStartError(undefined)
    try {
      const radar = await api.post<RadarView>(`/api/conversations/${conversationId}/radar/retry`)
      setState({ kind: 'loaded', radar })
      setRound((n) => n + 1)
    } catch (e) {
      setStartError(e)
      // 409 代表後端的狀態和畫面上的不同（例如已在計算或已完成）：重新讀取
      if (isApiError(e) && e.status === 409) setRound((n) => n + 1)
    } finally {
      busy.current = false
      setStarting(false)
    }
  }, [conversationId])

  return { state, check, start, starting, startError }
}
