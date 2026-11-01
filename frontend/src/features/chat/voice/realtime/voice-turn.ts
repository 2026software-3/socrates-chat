/**
 * 把語音回合接進對話串：學生訊息與回覆都已由後端存好（走 WebSocket），adapter 不必再 POST，
 * 只要把 WebSocket 收到的文字當成串流顯示。回合數、「思考中」與結束按鈕因此都不必另外處理。
 */
export type VoiceTurn = {
  push: (text: string) => void
  end: () => void
}

export type VoiceTurns = {
  /** 在把學生訊息加進對話串之前呼叫。 */
  begin: () => VoiceTurn
  /** adapter 開始一輪時取用；沒有表示這是文字訊息。 */
  take: () => AsyncIterable<string> | undefined
}

export function createVoiceTurns(): VoiceTurns {
  let waiting: AsyncIterable<string> | undefined
  return {
    begin() {
      const queue: string[] = []
      let ended = false
      let wake: (() => void) | undefined
      const notify = () => {
        wake?.()
        wake = undefined
      }
      waiting = {
        async *[Symbol.asyncIterator]() {
          for (;;) {
            const text = queue.shift()
            if (text !== undefined) yield text
            else if (ended) return
            else await new Promise<void>((resolve) => (wake = resolve))
          }
        },
      }
      return {
        push(text) {
          if (ended) return
          queue.push(text)
          notify()
        },
        end() {
          ended = true
          notify()
        },
      }
    },
    take() {
      const turn = waiting
      waiting = undefined
      return turn
    },
  }
}
