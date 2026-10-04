/** 後端 API 的資料型別。對照 docs/api/mvp-backend-api.md。 */

export type Roles = { admin: boolean; teacher: boolean; student: boolean }

export type Me = {
  id: string
  email: string
  display_name: string | null
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
  topic_id: string | null
  status: ActivityStatus
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

/** 論證主要依循的倫理學派（S-04.2 的 6 個維度，加上 other）。 */
export type Framework =
  | 'utilitarianism'
  | 'deontology'
  | 'virtue'
  | 'contractarianism'
  | 'care'
  | 'existentialism'
  | 'other'
/** 雷達圖的 6 個維度（順序即軸的順序，從正上方順時針）。 */
export const SCHOOLS = ['utilitarianism', 'deontology', 'virtue', 'contractarianism', 'care', 'existentialism'] as const

/** 各學派的分數（0 到滿分）；單場是整數，平均值有小數。 */
export type SchoolScores = Record<string, number>

export const FRAMEWORKS: readonly Framework[] = [
  'utilitarianism',
  'deontology',
  'virtue',
  'contractarianism',
  'care',
  'existentialism',
  'other',
]

export type SummaryView = {
  status: SummaryStatus
  stance: string | null
  reasons: string | null
  /** 沒有分類（舊資料或模型沒給）時為 null；管理者看到的是固定字串 `message` */
  /** 學生最終主張的一句短句（AI 寫的）；管理者看到的是固定字串 `message` */
  claim?: string | null
  framework?: string | null
  /** 6 個學派維度的分數（各 0–5）；沒有分數時為 null */
  framework_scores?: SchoolScores | null
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
  claim?: string | null
  framework?: string | null
}

/** 修課名單上的學生與帳號狀態（GET /api/students）。 */
export type StudentRow = {
  email: string
  display_name: string | null
  /** 已有帳號（登入過，或管理者匯入時建立了內建帳號） */
  has_account: boolean
  disabled: boolean
  completed_conversations: number
}

/** 分類 → 人數；`unclassified` 是沒有分類的總結。 */
export type Counts = Record<string, number>

/** 6 維雷達圖的平均分數（只平均有分數的總結）。 */
export type Radar = {
  scored: number
  average: SchoolScores
  max: number
}

export type Distribution = {
  completed: number
  frameworks: Counts
  radar: Radar
}

/** 一個主張群組：AI 把同題目下意思相近的主張歸成一群並命名（暫定）。 */
export type ClaimGroup = {
  /** 群組名稱（AI 命名）；`other` 為 true 時沒有名稱，顯示「其他」 */
  name: string | null
  other: boolean
  count: number
  /** 這群人的核心理由（AI 總結的文字，沒有姓名與對話原文） */
  reasons: string[]
  /** 這群人在 6 個學派上的平均分數 */
  radar: Radar
}

export type TopicDistribution = Distribution & {
  title: string
  claims: ClaimGroup[]
  /** 有主張但還沒歸群的場數 */
  ungrouped: number
}

export type PersonalDashboard = Distribution & {
  total_conversations: number
  total_turns: number
  recent: {
    conversation_id: string
    title: string
    ended_at: string | null
    stage: 1 | 2 | 3
    turn_count: number
    stance: string | null
    claim: string | null
    framework: string | null
    framework_scores: SchoolScores | null
  }[]
}

export type ClassDashboard = Distribution & {
  /** 管理者身分看不到分析結果（S-02.2），只有人數 */
  masked: boolean
  students_total: number
  students_participating: number
  topics: TopicDistribution[]
}

export type EmailList = { emails: string[] }

export type RosterImportResult = {
  added: number
  existing: number
  invalid: { line: number; value: string }[]
  /** 這次新建立的內建帳號與臨時密碼（只出現一次；教師匯入時為空或沒有這個欄位） */
  credentials?: { email: string; temporary_password: string }[]
}

/** SSE `done` 事件的內容。 */
export type StreamDone = {
  message_id: string
  question_type: string | null
  stage: 1 | 2 | 3
  suggest_end: boolean
}
