/**
 * 「AI 用量可能變了」的通知：任何可能呼叫 AI 的操作結束後發出（成功或失敗都算，失敗的呼叫後端會退回額度，
 * 但用量仍可能已經變過）。外框的用量顯示收到後重新載入 `GET /api/me/usage`。
 */
export const AI_USAGE_STALE_EVENT = 'ai-usage-stale'

export function markAiUsageStale() {
  window.dispatchEvent(new Event(AI_USAGE_STALE_EVENT))
}
