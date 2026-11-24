import type { Lang } from '@/i18n/define'
import type { Period } from '@/features/admin/diagnostics-model'

/** 後端契約（F-39.1／F-39.2）：`GET /api/admin/health`、`GET /api/admin/services`、`POST /api/admin/services/check`。 */

export const PERIOD_DAYS: Record<Period, number> = { '24h': 1, '7d': 7, '30d': 30 }

/** 設定檔裡的名稱與說明：單一字串，或以語言為鍵的對照。 */
export type ServiceText = string | Record<string, string>

export function serviceText(text: ServiceText, lang: Lang): string {
  if (typeof text === 'string') return text
  return text[lang] ?? text['zh-TW'] ?? Object.values(text)[0] ?? ''
}

export type Overall = 'healthy' | 'degraded' | 'unhealthy'

export type RequestHealth = { total: number; error_5xx_ratio: number; p95_ms: number }

export type HealthView = {
  status: Overall
  period_days: number
  database: { status: 'healthy' | 'unhealthy'; response_time_ms: number | null }
  services: { total: number; healthy: number; unhealthy: number; unknown: number }
  pool: { size: number; idle: number; max: number }
  process: { started_at: string; uptime_seconds: number; version: string }
  maintenance: boolean
  cleanup: { last_success_at: string | null }
  stats_available: boolean
  requests: RequestHealth | null
}

export type ServiceStatus = 'checking' | 'no_data' | 'healthy' | 'unhealthy'
export type ServiceFailure = 'connection_failed' | 'timeout' | 'non_2xx'

export type StatusEvent = {
  status: 'healthy' | 'unhealthy'
  occurred_at: string
  failure: ServiceFailure | null
}

export type ServiceRow = {
  name: string
  label: ServiceText
  impact: ServiceText
  target: string | null
  status: ServiceStatus
  failure: ServiceFailure | null
  response_time_ms: number | null
  checked_at: string | null
  last_healthy_at: string | null
  status_since: string | null
  availability: number | null
  recent_events: StatusEvent[]
}

/** 即時語音節點（S-19）：即時狀態，沒有歷史與可用率。 */
export type VoiceNodeRow = {
  name: string
  status: 'active' | 'draining' | 'disabled'
  healthy: boolean
  sessions: number
  max_sessions: number | null
}

export type ServicesView = {
  services: ServiceRow[]
  history_available: boolean
  period_days: number
  voice_nodes: VoiceNodeRow[]
}

/** 秒數 → `{ unit, n }`：只取最大的兩個單位以內的概略值。 */
export function splitDuration(seconds: number): { days: number; hours: number; minutes: number } {
  const s = Math.max(0, Math.floor(seconds))
  return { days: Math.floor(s / 86400), hours: Math.floor((s % 86400) / 3600), minutes: Math.floor((s % 3600) / 60) }
}
