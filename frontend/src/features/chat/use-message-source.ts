import { useMemo, useRef } from 'react'
import type { MessageSource } from '@/lib/types'

/**
 * 下一則學生訊息的輸入來源（文字或語音，S-05.4）：語音輸入後送出就記為語音，
 * `take` 取用後回到文字。不影響畫面，所以不用 state。
 */
export function useMessageSource() {
  const ref = useRef<MessageSource>('text')
  return useMemo(
    () => ({
      set: (s: MessageSource) => {
        ref.current = s
      },
      take: (): MessageSource => {
        const s = ref.current
        ref.current = 'text'
        return s
      },
    }),
    [],
  )
}
