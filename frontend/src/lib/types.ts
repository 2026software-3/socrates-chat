/** 後端 API 的資料型別。對照 docs/api/mvp-backend-api.md。 */

export type Roles = { admin: boolean; teacher: boolean; student: boolean }

export type Me = {
  id: string
  email: string
  display_name: string | null
  is_admin: boolean
  roles: Roles
}

export type Topic = {
  id: string
  title: string
  description: string
  category: string | null
  is_active: boolean
}

export type ConversationStatus = 'active' | 'ended'

export type Conversation = {
  id: string
  topic_id: string | null
  title: string
  description: string
  language: string
  status: ConversationStatus
  /** 追問階段 1 釐清 / 2 論證 / 3 挑戰 */
  stage: 1 | 2 | 3
  turn_count: number
  converge_ready: boolean
  created_at: string
  ended_at: string | null
}

export type MessageRole = 'student' | 'ai'
export type MessageSource = 'text' | 'speech-browser' | 'speech-openai'

export type Message = {
  id: string
  role: MessageRole
  content: string
  source: MessageSource
  question_type: string | null
  created_at: string
}

export type ConversationDetail = Conversation & { messages: Message[] }

/** 送出訊息的回應；`auto_ended` 表示達回合上限，對話已自動結束、總結產生中。 */
export type SentMessage = Message & { auto_ended: boolean }

export type SummaryStatus = 'pending' | 'ready' | 'failed'

export type SummaryView = {
  status: SummaryStatus
  stance: string | null
  reasons: string | null
  turning_points: string | null
  completed_at: string | null
}

export type TeacherSummary = {
  conversation_id: string
  title: string
  ended_at: string | null
  student_id: string
  student_name: string | null
  student_email: string
  stance: string | null
  reasons: string | null
  turning_points: string | null
}

export type EmailList = { emails: string[] }

export type RosterImportResult = {
  added: number
  existing: number
  invalid: { line: number; value: string }[]
}

/** SSE `done` 事件的內容。 */
export type StreamDone = {
  message_id: string
  question_type: string | null
  stage: 1 | 2 | 3
  suggest_end: boolean
}
