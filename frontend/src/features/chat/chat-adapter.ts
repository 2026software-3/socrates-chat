import type { ChatModelAdapter, ChatModelRunResult } from '@assistant-ui/react'
import { type ReplyEvent, sendMessage, streamReply } from '@/features/chat/sse'
import { api, isAbortError, isApiError } from '@/lib/api'
import type { ConversationDetail, MessageSource, SentMessage, StreamDone } from '@/lib/types'

export type ChatAdapterOptions = {
  conversationId: string
  /** 已存在伺服器上的學生訊息 id（載入的歷史）；這些訊息不會再 POST。 */
  persistedIds?: Iterable<string>
  /** 取出這則學生訊息的輸入來源（文字或語音）；沒提供就是文字。 */
  takeSource?: () => MessageSource
  /** 這一輪若是語音回合，回傳回覆文字；訊息已由後端保存，不再 POST。 */
  takeVoiceTurn?: () => AsyncIterable<string> | undefined
  onSent?: (sent: SentMessage) => void
  onDone?: (done: StreamDone) => void
  /** 後端回報對話已結束（例如在別的分頁結束）；畫面應切成唯讀。 */
  onEnded?: () => void
}


/**
 * 把後端「POST 訊息並以 SSE 串流回覆」的契約包成 assistant-ui 的 ChatModelAdapter。
 *
 * 重試語意：串流的第一個事件 `message` 代表學生訊息已保存，這時就記進 `persisted`；之後即使
 * 串流失敗（ai_unavailable），重試只會用 GET 重新開串流，不會再 POST（否則同一回合會重複計算）。
 * POST 在保存前就失敗（HTTP 錯誤）則訊息並未保存，重試時會再 POST。錯誤不丟出，而是以 incomplete/error 狀態（內容是錯誤碼）交給畫面翻譯。
 * 接回：`GET …/stream` 也會接上還在產生的回覆（學生離開又回到對話頁）；若回覆剛好在接上前存好
 * （nothing_to_reply），改取已保存的回覆。
 * 已知限制：POST 在收到 `message` 事件前連線中斷（network）時無法分辨伺服器是否已保存；後端沒有
 * idempotency key，重試會再 POST 一次。畫面層在失敗時鎖住輸入框，避免在未保存的訊息後再疊訊息。
 * 這裡不記錄任何訊息內容。
 */
export function createChatAdapter(options: ChatAdapterOptions): ChatModelAdapter {
  const persisted = new Set<string>(options.persistedIds ?? [])
  // 回覆已完整；callback 的錯誤不能讓這則訊息變成失敗
  const notifyDone = (done: StreamDone) => {
    try {
      options.onDone?.(done)
    } catch {
      // 忽略
    }
  }
  return {
    async *run({ messages, abortSignal }): AsyncGenerator<ChatModelRunResult, void> {
      const last = messages.at(-1)
      if (!last || last.role !== 'user') return
      const voiceTurn = options.takeVoiceTurn?.()
      if (voiceTurn) {
        persisted.add(last.id)
        let text = ''
        try {
          for await (const delta of voiceTurn) {
            if (abortSignal.aborted) return
            text += delta
            yield { content: [{ type: 'text', text }] }
          }
        } catch {
          // 學生訊息已保存，重試只會重開串流
          yield { status: { type: 'incomplete', reason: 'error', error: 'ai_unavailable' } }
        }
        return
      }
      const onlyStream = persisted.has(last.id)
      try {
        let text = ''
        const events: AsyncGenerator<ReplyEvent | { type: 'sent'; sent: SentMessage }> = onlyStream
          ? streamReply(options.conversationId, abortSignal)
          : sendMessage(
              options.conversationId,
              {
                content: last.content.flatMap((p) => (p.type === 'text' ? [p.text] : [])).join('\n'),
                source: options.takeSource?.() ?? 'text',
              },
              abortSignal,
            )
        for await (const ev of events) {
          if (ev.type === 'sent') {
            persisted.add(last.id)
            options.onSent?.(ev.sent)
          } else if (ev.type === 'delta') {
            text += ev.text
            yield { content: [{ type: 'text', text }] }
          } else {
            notifyDone(ev.done)
          }
        }
      } catch (e) {
        if (abortSignal.aborted || isAbortError(e)) return
        if (onlyStream && isApiError(e) && e.code === 'nothing_to_reply') {
          const saved = await savedReply(options.conversationId, abortSignal).catch(() => undefined)
          if (saved) {
            notifyDone(saved.done)
            yield { content: [{ type: 'text', text: saved.text }] }
            return
          }
        }
        if (isApiError(e) && e.code === 'conversation_ended') {
          options.onEnded?.()
          return
        }
        yield { status: { type: 'incomplete', reason: 'error', error: isApiError(e) ? e.code : 'internal' } }
      }
    },
  }
}

/** 已保存的最後一則 AI 回覆（最後一則不是 AI 訊息時為 undefined）。 */
async function savedReply(
  conversationId: string,
  signal: AbortSignal,
): Promise<{ text: string; done: StreamDone } | undefined> {
  const d = await api.get<ConversationDetail>(`/api/conversations/${conversationId}`, signal)
  const last = d.messages.at(-1)
  if (last?.role !== 'ai') return undefined
  return {
    text: last.content,
    done: { message_id: last.id, question_type: last.question_type, stage: d.stage, suggest_end: d.converge_ready },
  }
}
