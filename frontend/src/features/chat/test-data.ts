import type { Conversation, ConversationDetail, Message, RadarView, SummaryView } from '@/lib/types'

/** 測試用的合成資料（不含任何真實學生資料）。 */
export function makeConversation(overrides: Partial<Conversation> = {}): Conversation {
  return {
    id: 'c1',
    activity_id: null,
    activity_title: null,
    topic_id: 't1',
    title: '合成題目：電車難題',
    title_pending: false,
    description: '合成描述：該不該扳動拉桿？',
    language: 'zh-TW',
    status: 'active',
    stage: 1,
    turn_count: 3,
    converge_ready: false,
    created_at: '2026-03-05T08:00:00Z',
    updated_at: '2026-03-05T08:00:00Z',
    ended_at: null,
    is_archived: false,
    classroom_name: null,
    classroom_archived: false,
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
  overrides: Partial<Conversation> & { generating?: boolean; voice_realtime?: boolean; guiding_questions?: string[] } = {},
  messages: Message[] = [
    makeMessage({ id: 'm1', role: 'student', content: '我認為應該扳動拉桿。' }),
    makeMessage({ id: 'm2', role: 'ai', content: '你說的「應該」是依據什麼原則呢？', question_type: 'clarify' }),
  ],
): ConversationDetail {
  const { generating = false, voice_realtime = false, guiding_questions = [], ...conversation } = overrides
  return { ...makeConversation(conversation), messages, generating, voice_realtime, guiding_questions }
}

export function makeSummary(overrides: Partial<SummaryView> = {}): SummaryView {
  return {
    status: 'ready',
    stance: '合成立場：支持扳動拉桿',
    reasons: '合成理由：結果論',
    completed_at: '2026-03-05T09:00:00Z',
    translation_pending: false,
    ...overrides,
  }
}

/** 已完成的單場雷達圖；其他狀態用 `makeRadarState`。 */
export function makeRadar(overrides: Partial<RadarView> = {}): RadarView {
  return {
    scale: 'legacy',
    status: 'ready',
    retryable: false,
    scores: { d1: 5, d2: 0, d3: 3, d4: 1, d5: 0, d6: 2 },
    dimensions: [1, 2, 3, 4, 5, 6].map((n) => ({
      id: `d${n}`,
      name: { zh_tw: `合成傾向${n}`, en: `Synthetic tendency ${n}`, es: `Tendencia sintética ${n}` },
      description: { zh_tw: `合成說明${n}`, en: `Synthetic description ${n}`, es: `Descripción sintética ${n}` },
    })),
    rules_version: 'radar-scoring-v1',
    completed_at: '2026-03-05T09:00:05Z',
    ...overrides,
  }
}

/** 還沒有分數的雷達圖狀態。 */
export function makeRadarState(status: RadarView['status'], retryable = false): RadarView {
  return makeRadar({
    status,
    retryable,
    scores: null,
    rules_version: null,
    completed_at: null,
    ...(status === 'dimensions_pending' || status === 'unavailable' ? { dimensions: null } : {}),
  })
}
