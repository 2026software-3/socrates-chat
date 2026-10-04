import type { Conversation, ConversationDetail, Message, SummaryView } from '@/lib/types'

/** 測試用的合成資料（不含任何真實學生資料）。 */
export function makeConversation(overrides: Partial<Conversation> = {}): Conversation {
  return {
    id: 'c1',
    activity_id: null,
    topic_id: 't1',
    title: '合成題目：電車難題',
    description: '合成描述：該不該扳動拉桿？',
    language: 'zh-TW',
    status: 'active',
    stage: 1,
    turn_count: 3,
    converge_ready: false,
    created_at: '2026-03-05T08:00:00Z',
    ended_at: null,
    ...overrides,
  }
}

export function makeMessage(overrides: Partial<Message> = {}): Message {
  return {
    id: 'm1',
    role: 'student',
    content: '合成的學生訊息',
    source: 'text',
    question_type: null,
    created_at: '2026-03-05T08:01:00Z',
    ...overrides,
  }
}

export function makeDetail(
  overrides: Partial<Conversation> = {},
  messages: Message[] = [
    makeMessage({ id: 'm1', role: 'student', content: '我認為應該扳動拉桿。' }),
    makeMessage({ id: 'm2', role: 'ai', content: '你說的「應該」是依據什麼原則呢？', question_type: 'clarify' }),
  ],
): ConversationDetail {
  return { ...makeConversation(overrides), messages }
}

export function makeSummary(overrides: Partial<SummaryView> = {}): SummaryView {
  return {
    status: 'ready',
    stance: '合成立場：支持扳動拉桿',
    reasons: '合成理由：結果論',
    completed_at: '2026-03-05T09:00:00Z',
    ...overrides,
  }
}
