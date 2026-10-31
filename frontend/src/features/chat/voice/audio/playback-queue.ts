/**
 * AI 語音的播放佇列（S-05.5）。一次只服務一則回覆，打斷後立刻丟掉所有排隊中的片段，
 * 遲到的片段因為屬於已結束的回覆而被拒收，不可能播出。
 *
 * 回覆邊生成邊加入片段，佇列播完時若回覆還沒結束就等下一段。
 * 片段型別不限：目前是一句文字，日後串流 TTS 可以換成音訊片段，規則不變。
 */

export type PlaybackOutcome = 'finished' | 'interrupted'

/** 播放一個片段；`signal` 中止時必須立刻停止發聲。 */
export type PlayChunk<C> = (chunk: C, signal: AbortSignal) => Promise<void>

export type PlaybackQueueOptions<C> = {
  /** 播放前把排隊中的片段併進目前片段；回傳 `undefined` 表示不合併。 */
  merge?: (chunk: C, next: C) => C | undefined
}

export type PlaybackQueue<C> = {
  /** 開始一則回覆並回傳它的結局；還在播的回覆會先被打斷，播放失敗時 reject。 */
  begin: (responseId: string) => Promise<PlaybackOutcome>
  /** 加入片段；不是目前的回覆就回傳 `false`，片段被丟棄。 */
  enqueue: (responseId: string, chunk: C) => boolean
  /** 標記這則回覆不會再有片段，回傳它的結局。 */
  finish: (responseId: string) => Promise<PlaybackOutcome>
  /** 立即中止目前的回覆。 */
  interrupt: () => void
}

type Turn<C> = {
  id: string
  chunks: C[]
  closed: boolean
  pumping: boolean
  ctrl: AbortController
  outcome: Promise<PlaybackOutcome>
  settle: (o: PlaybackOutcome) => void
  fail: (e: unknown) => void
}

export function createPlaybackQueue<C>(play: PlayChunk<C>, { merge }: PlaybackQueueOptions<C> = {}): PlaybackQueue<C> {
  let current: Turn<C> | undefined

  function nextChunk(chunks: C[]): C {
    let chunk = chunks.shift() as C
    while (merge && chunks.length > 0) {
      const merged = merge(chunk, chunks[0])
      if (merged === undefined) break
      chunk = merged
      chunks.shift()
    }
    return chunk
  }

  async function pump(turn: Turn<C>) {
    if (turn.pumping) return
    turn.pumping = true
    try {
      while (current === turn && turn.chunks.length > 0) {
        await play(nextChunk(turn.chunks), turn.ctrl.signal)
      }
    } catch (e) {
      // 已被打斷的回覆，它的播放錯誤沒有意義；結局在打斷時就決定了
      if (current === turn) {
        current = undefined
        turn.fail(e)
      }
      return
    } finally {
      turn.pumping = false
    }
    if (current === turn && turn.closed) {
      current = undefined
      turn.settle('finished')
    }
  }

  function interrupt() {
    const turn = current
    if (!turn) return
    current = undefined
    turn.chunks.length = 0
    turn.ctrl.abort()
    turn.settle('interrupted')
  }

  return {
    begin(responseId) {
      interrupt()
      let settle: Turn<C>['settle'] = () => {}
      let fail: Turn<C>['fail'] = () => {}
      const outcome = new Promise<PlaybackOutcome>((resolve, reject) => {
        settle = resolve
        fail = reject
      })
      // 沒有人等結局就失敗時，不要變成 unhandled rejection
      outcome.catch(() => {})
      current = { id: responseId, chunks: [], closed: false, pumping: false, ctrl: new AbortController(), outcome, settle, fail }
      return outcome
    },
    enqueue(responseId, chunk) {
      if (current?.id !== responseId) return false
      current.chunks.push(chunk)
      void pump(current)
      return true
    },
    finish(responseId) {
      const turn = current
      if (turn?.id !== responseId) return Promise.resolve('interrupted')
      turn.closed = true
      if (turn.chunks.length === 0 && !turn.pumping) {
        current = undefined
        turn.settle('finished')
      }
      return turn.outcome
    },
    interrupt,
  }
}
