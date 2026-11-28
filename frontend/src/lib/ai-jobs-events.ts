/**
 * 「可能有新的 AI 工作開始了」的通知：結束對話、重試等會在背景啟動 AI 工作的操作完成後發出，
 * 全域進度顯示收到後立刻重新查一次進行中的工作（S-20 §4.1）。
 */
export const AI_JOBS_WAKE_EVENT = 'ai-jobs-wake'

export function wakeAiJobs() {
  window.dispatchEvent(new Event(AI_JOBS_WAKE_EVENT))
}
