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
  /** 有內建密碼且不是示範帳號：個人設定才顯示「更改密碼」（S-18） */
  can_change_password: boolean
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
  /** 活動的題目清單（1 到 5 個，依教師排定的順序；S-04.7）。 */
  topic_ids: string[]
  opens_at: string | null
  closes_at: string | null
  classroom_id: string
  status: ActivityStatus
  /** 已封存：學生看不到、不能開新對話，可復原 */
  is_archived: boolean
  created_at: string
}

/** 學生在活動中某一題的進度（F-29.6）。 */
export type TopicProgress = {
  topic_id: string
  title: string
  /** 這位學生在此題的對話；還沒開始為 null */
  conversation_id: string | null
  conversation_status: ConversationStatus | null
  /** 此題總結已產生（S-04.7：完成一題的條件） */
  summary_ready: boolean
}

/** 學生看到的活動：每題進度與整體完成度（每題都有總結才算完成）。 */
export type StudentActivity = Activity & { topics: TopicProgress[]; completed: boolean }

export type Available = { activities: StudentActivity[]; topics: Topic[] }

export type ConversationStatus = 'active' | 'ended'

export type Conversation = {
  id: string
  activity_id: string | null
  /** 活動對話所屬活動目前的名稱；個人練習為 null */
  activity_title: string | null
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
  updated_at: string
  ended_at: string | null
  /** 學生已封存：只從自己的清單隱藏，可復原 */
  is_archived: boolean
}

export type MessageRole = 'student' | 'ai'
export type MessageSource = 'text' | 'speech-browser' | 'speech-openai' | 'speech-cascade'

export type Message = {
  id: string
  role: MessageRole
  content: string
  source: MessageSource
  question_type: string | null
  created_at: string
}

/** 對話詳情；`generating` 表示 AI 回覆正在產生，可用 `GET …/stream` 接上。 */
export type ConversationDetail = Conversation & {
  messages: Message[]
  generating: boolean
  /** 伺服器端語音對話已啟用（`GET /api/conversations/{id}/voice`） */
  voice_realtime: boolean
}

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
  translation?: SummaryTranslation | null
  translation_pending?: boolean
}

/** 教師與管理者的總結列表項目（GET /api/summaries，分頁）。 */
export type TeacherSummary = {
  conversation_id: string
  /** 對話所屬的教室 */
  classroom_id: string
  /** 對話所屬的活動 ID；個人練習為 null */
  activity_id: string | null
  /** 對話所屬的題目 ID；個人練習為 null，題目被刪除後也會變成 null */
  topic_id: string | null
  /** 該學生是否已完成該活動內的全部題目（S-04.7、F-29.3） */
  activity_completed: boolean
  title: string
  ended_at: string
  /** 學生的使用者 UUID（`?student=` 篩選與學生頁連結用這個），不是學號 */
  student_user_id: string
  student_name: string | null
  student_email: string
  /** 對話建立時快照的原文語言。 */
  language: Lang
  stance: string | null
  reasons: string | null
  claim?: string | null
  translation?: SummaryTranslation | null
  translation_pending?: boolean
}

/** 教師唯讀檢視的一場已結束活動對話（GET /api/summaries/{id}/messages）；管理者的 content 一律是 `message`。 */
export type Transcript = {
  conversation_id: string
  classroom_id: string
  title: string
  language: Lang
  ended_at: string
  /** 學生的使用者 UUID，不是學號 */
  student_user_id: string
  student_name: string | null
  messages: Message[]
}

/** 已保存或按需產生的總結譯文。 */
export type SummaryTranslation = {
  language: Lang
  stance: string
  reasons: string
}

/** 教室修課名單上的學生與帳號狀態（GET /api/classrooms/{id}/students）。 */
export type StudentRow = {
  id: string | null
  email: string | null
  /** 學號（大寫）；學生的帳號 UUID 是 `id` */
  student_number: string
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
  credentials?: { student_number: string; temporary_password: string }[]
}

/** SSE `done` 事件的內容。 */
export type StreamDone = {
  message_id: string
  question_type: string | null
  stage: 1 | 2 | 3
  suggest_end: boolean
  /** AI 這則是結束語：討論已結束、總結產生中。 */
  ended?: boolean
}

/** Keyset 分頁列表。 */
export type Page<T> = { items: T[]; next_cursor: string | null }

/** 學生列表的一頁；`total` 是符合搜尋條件的學生總數，每一頁都帶。 */
export type StudentPage = Page<StudentRow> & { total: number }

/** `GET /api/me/usage`：滑動時間窗內自己的 AI 用量。 */
export type AiUsage = {
  used: number
  limit: number
  window_secs: number
  /** 最早那一次呼叫還要幾秒滑出時間窗（多久後會空出一個額度）；尚未使用時為 null。 */
  resets_in_secs: number | null
}
