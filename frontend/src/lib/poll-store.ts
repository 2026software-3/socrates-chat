import { isAbortError } from '@/lib/api'

/**
 * 下一步：`done` 停止；`poll` 一直輪詢；`poll-limited` 輪詢，但從開始算起超過 `maxMs` 就視為逾時。
 * 逾時只代表「畫面不再等」，工作本身由後端繼續（S-20）。
 */
export type PollNext = 'done' | 'poll' | 'poll-limited'

export type PollSpec<T> = {
  fetch: (signal: AbortSignal) => Promise<T>
  next: (data: T) => PollNext
  intervalMs: number
  maxMs: number
}

export type PollState<T> = {
  data: T | undefined
  error: unknown
  timedOut: boolean
  polling: boolean
}

const IDLE: PollState<never> = { data: undefined, error: undefined, timedOut: false, polling: false }

type Entry = {
  state: PollState<unknown>
  spec: PollSpec<unknown>
  ctrl?: AbortController
  timer?: ReturnType<typeof setTimeout>
}

/**
 * 輪詢工作的全域登記處（S-20 §4.1）：輪詢由這裡持有，不綁元件生命週期，
 * 元件切頁或關閉對話框只是取消訂閱，輪詢照常進行；回來時直接讀到目前的狀態。
 */
export class PollStore {
  private entries = new Map<string, Entry>()
  private listeners = new Set<() => void>()

  subscribe = (listener: () => void) => {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  get<T>(key: string | null): PollState<T> {
    return ((key !== null && this.entries.get(key)?.state) || IDLE) as PollState<T>
  }

  /** 仍在輪詢的 key。 */
  pollingKeys(): string[] {
    return [...this.entries].filter(([, e]) => e.state.polling).map(([k]) => k)
  }

  /**
   * 開始輪詢 `key`。已在輪詢就沿用（不開第二個迴圈）；`refresh` 時，已停止的登記會重新讀取一次。
   * 已有結果但沒有 `refresh` 時不重打。
   */
  watch<T>(key: string, spec: PollSpec<T>, { refresh = false }: { refresh?: boolean } = {}) {
    const existing = this.entries.get(key)
    if (existing) {
      existing.spec = spec as PollSpec<unknown>
      if (existing.state.polling || !refresh) return
      this.run(existing)
      return
    }
    const entry: Entry = { state: IDLE, spec: spec as PollSpec<unknown> }
    this.entries.set(key, entry)
    this.run(entry)
  }

  /** 重新開始（逾時、失敗後再檢查，或重試）；可用 `data` 先塞入剛從 POST 回來的結果。 */
  restart<T>(key: string, { data, spec }: { data?: T; spec?: PollSpec<T> } = {}) {
    const entry = this.entries.get(key)
    if (!entry) return
    if (spec) entry.spec = spec as PollSpec<unknown>
    this.run(entry, data === undefined ? { data: undefined } : { data })
  }

  /** 登出等情況：中止所有輪詢並清空。 */
  dispose() {
    for (const entry of this.entries.values()) this.halt(entry)
    this.entries.clear()
    this.emit()
  }

  private halt(entry: Entry) {
    entry.ctrl?.abort()
    clearTimeout(entry.timer)
  }

  private set(entry: Entry, patch: Partial<PollState<unknown>>) {
    entry.state = { ...entry.state, ...patch }
    this.emit()
  }

  private emit() {
    for (const l of [...this.listeners]) l()
  }

  private run(entry: Entry, seed?: { data: unknown }) {
    this.halt(entry)
    const ctrl = new AbortController()
    entry.ctrl = ctrl
    const startedAt = Date.now()
    this.set(entry, { data: seed ? seed.data : entry.state.data, error: undefined, timedOut: false, polling: true })

    const tick = async () => {
      const { spec } = entry
      try {
        const data = await spec.fetch(ctrl.signal)
        if (ctrl.signal.aborted) return
        const next = spec.next(data)
        if (next === 'done') this.set(entry, { data, polling: false })
        else if (next === 'poll-limited' && Date.now() - startedAt >= spec.maxMs) {
          this.set(entry, { data, polling: false, timedOut: true })
        } else {
          this.set(entry, { data })
          entry.timer = setTimeout(() => void tick(), spec.intervalMs)
        }
      } catch (e) {
        if (ctrl.signal.aborted || isAbortError(e)) return
        this.set(entry, { error: e, polling: false })
      }
    }
    void tick()
  }
}
