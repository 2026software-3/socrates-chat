import { useCallback, useEffect, useRef } from 'react'

/**
 * 給「維持同步」的單次請求用（金鑰測試等，S-20 §4.3）：元件卸載時中止還在等的請求，
 * 呼叫端在 `signal.aborted` 時不要再更新畫面或跳通知。
 */
export function useAbortOnUnmount() {
  const ref = useRef<AbortController>(undefined)
  useEffect(() => {
    ref.current = new AbortController()
    return () => ref.current?.abort()
  }, [])
  return useCallback(() => ref.current?.signal, [])
}
