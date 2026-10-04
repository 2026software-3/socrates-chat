import type { ChatModelAdapter, ChatModelRunResult } from '@assistant-ui/react'
import { streamReply } from '@/features/chat/sse'
import { api, isApiError } from '@/lib/api'
import type { MessageSource, SentMessage, StreamDone } from '@/lib/types'

export type ChatAdapterOptions = {
  conversationId: string
  /** 已存在伺服器上的學生訊息 id（載入的歷史）；這些訊息不會再 POST。 */
  persistedIds?: Iterable<string>
  /** 取出這則學生訊息的輸入來源（文字或語音）；沒提供就是文字。 */
  takeSource?: () => MessageSource
  onSent?: (sent: SentMessage) => void
  onDone?: (done: StreamDone) => void
  /** 後端回報對話已結束（例如在別的分頁結束）；畫面應切成唯讀。 */
  onEnded?: () => void
}

function isAbort(e: unknown): boolean {
  return e instanceof DOMException && e.name === 'AbortError'
}

/**
 * 把後端「POST 訊息 → 開 SSE」的契約包成 assistant-ui 的 ChatModelAdapter。
 *
 * 重試語意：學生訊息一旦 POST 成功就記進 `persisted`；之後即使串流失敗（ai_unavailable），
 * 重試只會重新開串流，不會再 POST（否則同一回合會重複計算）。POST 失敗則訊息並未保存，
 * 重試時會再 POST。錯誤不丟出，而是以 incomplete/error 狀態（內容是錯誤碼）交給畫面翻譯。
 * 已知限制：POST 回應遺失（network）時無法分辨伺服器是否已保存；後端沒有 idempotency key，
 * 重試會再 POST 一次。畫面層在失敗時鎖住輸入框，避免在未保存的訊息後再疊訊息。
 * 這裡不記錄任何訊息內容。
 */
export function createChatAdapter(options: ChatAdapterOptions): ChatModelAdapter {
  const persisted = new Set<string>(options.persistedIds ?? [])
  return {
    async *run({ messages, abortSignal }): AsyncGenerator<ChatModelRunResult, void> {
      const last = messages.at(-1)
      if (!last || last.role !== 'user') return
      try {
        if (!persisted.has(last.id)) {
          const content = last.content.flatMap((p) => (p.type === 'text' ? [p.text] : [])).join('\n')
          const source = options.takeSource?.() ?? 'text'
          const sent = await api.post<SentMessage>(`/api/conversations/${options.conversationId}/messages`, {
            content,
            source,
          })
          persisted.add(last.id)
          options.onSent?.(sent)
          // 達回合上限：對話已自動結束，沒有 AI 回覆可串流
          if (sent.auto_ended) return
        }
        if (abortSignal.aborted) return
        let text = ''
        for await (const ev of streamReply(options.conversationId, abortSignal)) {
          if (ev.type === 'delta') {
            text += ev.text
            yield { content: [{ type: 'text', text }] }
          } else {
            // 回覆已完整；callback 的錯誤不能讓這則訊息變成失敗
            try {
              options.onDone?.(ev.done)
            } catch {
              // 忽略
            }
          }
        }
      } catch (e) {
        if (abortSignal.aborted || isAbort(e)) return
        if (isApiError(e) && e.code === 'conversation_ended') {
          options.onEnded?.()
          return
        }
        yield { status: { type: 'incomplete', reason: 'error', error: isApiError(e) ? e.code : 'internal' } }
      }
    },
  }
}
