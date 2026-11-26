import { useCallback, useMemo, useState } from 'react'
import { useAccountLanguage } from '@/auth/auth-context'
import { api, isApiError, type ApiError } from '@/lib/api'
import { usePollJob } from '@/lib/ai-jobs'
import { wakeAiJobs } from '@/lib/ai-jobs-events'
import type { PollSpec } from '@/lib/poll-store'
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
 * 總結 ready 後仍會在翻譯待完成時繼續輪詢；總結產生超過 `maxPollMs` 顯示 timeout。
 * 輪詢由全域登記處持有（S-20）：離開頁面不會中止，回來時接上同一個工作。
 */
export function useSummary(conversationId: string, { pollMs = SUMMARY_POLL_MS, maxPollMs = SUMMARY_MAX_POLL_MS }: Options = {}) {
  const accountLanguage = useAccountLanguage()
  const [retrying, setRetrying] = useState(false)
  const [retryError, setRetryError] = useState<unknown>()

  const spec = useMemo<PollSpec<SummaryView>>(
    () => ({
      fetch: (signal) => api.get<SummaryView>(`/api/conversations/${conversationId}/summary`, signal),
      next: (s) => (s.status === 'ready' ? (s.translation_pending ? 'poll' : 'done') : s.status === 'failed' ? 'done' : 'poll-limited'),
      intervalMs: pollMs,
      maxMs: maxPollMs,
    }),
    [conversationId, pollMs, maxPollMs],
  )
  // 帳號語言進 key：換語言後總結的顯示語言不同，要重新讀取
  const { state: poll, restart } = usePollJob(accountLanguage ? `summary:${conversationId}:${accountLanguage}` : null, spec)

  const state: SummaryState = poll.error !== undefined
    ? { kind: 'error', error: isApiError(poll.error) ? poll.error : undefined }
    : poll.data?.status === 'ready' ? { kind: 'ready', summary: poll.data }
    : poll.data?.status === 'failed' ? { kind: 'failed' }
    : poll.timedOut ? { kind: 'timeout' }
    : { kind: 'pending' }

  /** 重新開始輪詢（逾時或查詢失敗後的「再檢查一次」）。 */
  const check = useCallback(() => restart({}), [restart])

  /** 總結失敗後重新產生（POST summary/retry），成功後回到輪詢。 */
  const retry = useCallback(async () => {
    setRetrying(true)
    setRetryError(undefined)
    try {
      await api.post(`/api/conversations/${conversationId}/summary/retry`)
      wakeAiJobs()
      check()
    } catch (e) {
      setRetryError(e)
    } finally {
      setRetrying(false)
    }
  }, [conversationId, check])

  return { state, check, retry, retrying, retryError }
}
