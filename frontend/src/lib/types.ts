/** 後端 API 的資料型別。對照 docs/api/api-reference.md。 */

import type { Lang } from '@/i18n/define'

export type Roles = { admin: boolean; teacher: boolean; student: boolean }

export type Me = {
  id: string
  email: string | null
  display_name: string | null
  language: Lang
  is_admin: boolean
  /** 用臨時密碼登入、尚未改密碼；其他 API 會回 403 `password_change_required` */
  must_change_password: boolean
  roles: Roles
}

export type Topic = {
  id: string
  title: string
  description: string
  category: string | null
  is_active: boolean
}

export type ActivityStatus = 'draft' | 'published' | 'closed'

export type Activity = {
  id: string
  title: string
  description: string
  /** 活動必須選題庫題目（建立與編輯都必填）。 */
  topic_id: string
  opens_at: string | null
  closes_at: string | null
  classroom_id: string
  status: ActivityStatus
  /** 已封存：學生看不到、不能開新對話，可復原 */
  is_archived: boolean
  created_at: string
}

export type Available = { activities: Activity[]; topics: Topic[] }

export type ConversationStatus = 'active' | 'ended'

export type Conversation = {
  id: string
  activity_id: string | null
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
  /** 學生已封存：只從自己的清單隱藏，可復原 */
  is_archived: boolean
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

/** 對話詳情；`generating` 表示 AI 回覆正在產生，可用 `GET …/stream` 接上。 */
export type ConversationDetail = Conversation & { messages: Message[]; generating: boolean }

/** 送出訊息的回應；`auto_ended` 表示達回合上限，對話已自動結束、總結產生中。 */
export type SentMessage = Message & { auto_ended: boolean }

export type SummaryStatus = 'pending' | 'ready' | 'failed'

export type SummaryView = {
  status: SummaryStatus
  stance: string | null
  reasons: string | null
  /** 學生最終主張的一句短句（AI 寫的）；沒有時為 null；管理者看到的是固定字串 `message` */
  claim?: string | null
  completed_at: string | null
}

/** 教師與管理者的總結列表項目（GET /api/summaries，分頁）。 */
export type TeacherSummary = {
  conversation_id: string
  /** 對話所屬的教室 */
  classroom_id: string
  title: string
  ended_at: string
  student_id: string
  student_name: string | null
  student_email: string
  /** 對話建立時快照的原文語言。 */
  language: Lang
  stance: string | null
  reasons: string | null
  claim?: string | null
}

/** 教師明確觸發、僅保留在目前頁面的總結翻譯。 */
export type SummaryTranslation = {
  language: Lang
  stance: string
  reasons: string
}

/** 教室修課名單上的學生與帳號狀態（GET /api/classrooms/{id}/students）。 */
export type StudentRow = {
  id: string | null
  email: string | null
  student_id: string
  display_name: string | null
  /** 已有帳號（登入過，或管理者匯入時建立了內建帳號） */
  has_account: boolean
  disabled: boolean
  completed_conversations: number
}

/** 一個主張群組：AI 把同活動下意思相近的主張歸成一群並命名（暫定）。 */
export type ClaimGroup = {
  /** 群組名稱（AI 命名）；`other` 為 true 時沒有名稱，顯示「其他」 */
  name: string | null
  other: boolean
  count: number
  /** 這群人的核心理由（AI 總結的文字，沒有姓名與對話原文） */
  reasons: string[]
}

/** 教師班級儀表板中的一個活動（以 `activity_id` 識別）。 */
export type ActivityDistribution = {
  activity_id: string
  title: string
  completed: number
  claims: ClaimGroup[]
  /** 有主張但還沒歸群的場數 */
  ungrouped: number
}

export type PersonalDashboard = {
  total_conversations: number
  total_turns: number
  completed: number
  recent: {
    conversation_id: string
    title: string
    ended_at: string | null
    stage: 1 | 2 | 3
    turn_count: number
    stance: string | null
    claim: string | null
  }[]
}

export type ClassDashboard = {
  /** 管理者身分看不到分析結果（S-02.2），只有人數 */
  masked: boolean
  students_total: number
  students_participating: number
  /** 全部已完成場數 */
  completed: number
  activities: ActivityDistribution[]
}

export type EmailList = { emails: string[] }

export type Classroom = {
  is_archived: boolean
  id: string
  name: string
  year_label: string | null
  description: string | null
  created_at: string
  updated_at: string
}

export type ImportErrorReason = 'missing_fields' | 'invalid_format'

export type RosterImportResult = {
  added: number
  existing: number
  invalid: { line: number; reason: ImportErrorReason }[]
  /** 這次新建立的內建帳號與臨時密碼（只出現一次；教師匯入時為空或沒有這個欄位） */
  credentials?: { student_id: string; temporary_password: string }[]
}

/** SSE `done` 事件的內容。 */
export type StreamDone = {
  message_id: string
  question_type: string | null
  stage: 1 | 2 | 3
  suggest_end: boolean
}

/** Keyset 分頁列表。 */
export type Page<T> = { items: T[]; next_cursor: string | null }

/** 學生列表的一頁；`total` 是符合搜尋條件的學生總數，每一頁都帶。 */
export type StudentPage = Page<StudentRow> & { total: number }
