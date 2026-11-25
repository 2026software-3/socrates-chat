import { createContext, useCallback, useContext, useEffect, useRef, useState, useSyncExternalStore, type ReactNode } from 'react'
import { PollStore, type PollSpec, type PollState } from '@/lib/poll-store'

const AiJobsContext = createContext<PollStore | null>(null)

/**
 * 持有所有 AI 工作的輪詢（S-20 §4.1）。放在 App 層，切頁、關閉對話框都不會中止輪詢；
 * 只有 provider 卸載，或 `userKey`（登入者）改變時才會中止並清空。
 */
export function AiJobsProvider({ userKey, children }: { userKey?: string; children: ReactNode }) {
  const [store] = useState(() => new PollStore())
  // 換人或登出時清掉上一位使用者的工作與結果
  useEffect(() => () => store.dispose(), [store, userKey])
  return <AiJobsContext.Provider value={store}>{children}</AiJobsContext.Provider>
}

/** 全域登記處；沒有 provider（例如單獨渲染的元件測試）時回傳 `null`。 */
export function useAiJobsStore(): PollStore | null {
  return useContext(AiJobsContext)
}

/**
 * 訂閱一個輪詢工作。`key` 為 `null` 時不輪詢。輪詢在全域登記處進行，元件卸載只是取消訂閱；
 * 元件掛載時若同一 key 還在輪詢就直接接上，已停止則重新讀取一次。
 * 沒有 provider 時退回元件自己的登記處，卸載即中止。
 */
export function usePollJob<T>(key: string | null, spec: PollSpec<T>) {
  const global = useAiJobsStore()
  const [local] = useState(() => (global ? null : new PollStore()))
  const store = global ?? local!
  const specRef = useRef(spec)
  specRef.current = spec

  useEffect(() => {
    if (key !== null) store.watch(key, specRef.current, { refresh: true })
  }, [store, key])
  useEffect(() => () => local?.dispose(), [local])

  const state = useSyncExternalStore(store.subscribe, () => store.get<T>(key)) as PollState<T>

  /** 重新開始輪詢；`data` 可先塞入剛從 POST 回來的結果，`spec` 可換成另一個讀取方式（例如重試）。 */
  const restart = useCallback(
    (opts: { data?: T; spec?: PollSpec<T> } = {}) => {
      if (key !== null) store.restart(key, { spec: specRef.current, ...opts })
    },
    [store, key],
  )
  return { state, restart }
}
