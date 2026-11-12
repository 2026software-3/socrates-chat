import type { MessageKey } from '@/i18n'

/** 後端診斷 API 的契約（F-33.1，`GET /api/admin/status`、`GET /api/admin/diagnostics`）。 */
export type DiagnosticKind = 'ai' | 'login' | 'account_lock'

export type FailureClass =
  | 'connection_failed'
  | 'timeout'
  | 'invalid_key'
  | 'quota_exceeded'
  | 'rate_limited'
  | 'service_error'
  | 'invalid_output'
  | 'invalid_credentials'
  | 'account_disabled'
  | 'account_locked'

export type DiagnosticEvent = {
  occurred_at: string
  request_id: string
  kind: DiagnosticKind
  failure_class: FailureClass
  function: string
  route: string
  status: number
  error_code: string
  user_id: string | null
  classroom_id: string | null
  duration_ms: number | null
  retry_count: number | null
  model: string | null
}

export type Period = '24h' | '7d' | '30d'
export const PERIODS: Period[] = ['24h', '7d', '30d']

export type AiStatus = 'no_data' | 'ok' | 'has_failures'

export type Statistics = {
  ai_status: AiStatus
  ai_calls: number
  ai_failures: number
  ai_invalid_outputs: number
  login_failures: number
  account_locks: number
}

export type SystemStatus = {
  service: string
  database: string
  period: Period
  diagnostics_available: boolean
  statistics: Statistics | null
  recent_errors: DiagnosticEvent[]
}

export const KINDS: DiagnosticKind[] = ['ai', 'login', 'account_lock']

/** 每種事件類型可能出現的失敗分類，用來限縮篩選選項。 */
export const CLASSES_BY_KIND: Record<DiagnosticKind, FailureClass[]> = {
  ai: [
    'connection_failed',
    'timeout',
    'invalid_key',
    'quota_exceeded',
    'rate_limited',
    'service_error',
    'invalid_output',
  ],
  login: ['invalid_credentials', 'account_disabled'],
  account_lock: ['account_locked'],
}

export const ALL_CLASSES: FailureClass[] = [
  ...CLASSES_BY_KIND.ai,
  ...CLASSES_BY_KIND.login,
  ...CLASSES_BY_KIND.account_lock,
]

export const KIND_LABEL: Record<DiagnosticKind, MessageKey> = {
  ai: 'admin.diag.kind.ai',
  login: 'admin.diag.kind.login',
  account_lock: 'admin.diag.kind.account_lock',
}

export const CLASS_LABEL: Record<FailureClass, MessageKey> = {
  connection_failed: 'admin.diag.class.connection_failed',
  timeout: 'admin.diag.class.timeout',
  invalid_key: 'admin.diag.class.invalid_key',
  quota_exceeded: 'admin.diag.class.quota_exceeded',
  rate_limited: 'admin.diag.class.rate_limited',
  service_error: 'admin.diag.class.service_error',
  invalid_output: 'admin.diag.class.invalid_output',
  invalid_credentials: 'admin.diag.class.invalid_credentials',
  account_disabled: 'admin.diag.class.account_disabled',
  account_locked: 'admin.diag.class.account_locked',
}

export const FUNCTION_LABEL: Record<string, MessageKey> = {
  chat_reply: 'admin.diag.fn.chat_reply',
  summary: 'admin.diag.fn.summary',
  summary_translation: 'admin.diag.fn.summary_translation',
  claim_group: 'admin.diag.fn.claim_group',
  claim_regroup: 'admin.diag.fn.claim_regroup',
  achievement_translation: 'admin.diag.fn.achievement_translation',
  voice_transcription: 'admin.diag.fn.voice_transcription',
  voice_synthesis: 'admin.diag.fn.voice_synthesis',
  password_login: 'admin.diag.fn.password_login',
  google_login: 'admin.diag.fn.google_login',
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

export function isUuid(value: string): boolean {
  return UUID.test(value)
}

/** 診斷頁的篩選條件；每個欄位都是網址上的查詢參數，所以能從別的頁面直接連過來。 */
export type Filters = {
  from: string
  to: string
  kind: '' | DiagnosticKind
  failure_class: '' | FailureClass
  request_id: string
  user_id: string
  classroom_id: string
}

export const EMPTY_FILTERS: Filters = {
  from: '',
  to: '',
  kind: '',
  failure_class: '',
  request_id: '',
  user_id: '',
  classroom_id: '',
}

const FILTER_KEYS = Object.keys(EMPTY_FILTERS) as (keyof Filters)[]

export function filtersFromParams(params: URLSearchParams): Filters {
  const next = { ...EMPTY_FILTERS }
  for (const key of FILTER_KEYS) {
    const value = params.get(key)
    if (value) (next as Record<string, string>)[key] = value
  }
  if (next.kind && !KINDS.includes(next.kind)) next.kind = ''
  if (next.failure_class && !ALL_CLASSES.includes(next.failure_class)) next.failure_class = ''
  return next
}

export function filtersToParams(filters: Filters): URLSearchParams {
  const params = new URLSearchParams()
  for (const key of FILTER_KEYS) if (filters[key]) params.set(key, filters[key])
  return params
}

/** `<input type="datetime-local">` 的值（本地時間）轉成 API 用的 RFC 3339；空白或無法解析回傳 `null`。 */
export function localToIso(value: string): string | null {
  if (!value) return null
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? null : date.toISOString()
}

/** 網址上的 RFC 3339 轉回 `datetime-local` 的值。 */
export function isoToLocal(value: string): string {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return ''
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`
}
