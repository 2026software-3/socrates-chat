/**
 * 對話頁標題下顯示該題目的其中一個引導問題（S-04.8）。
 * 以對話 id 的雜湊決定要顯示哪一題：不同對話看到的題目隨機分散，
 * 但同一場對話每次打開、重新整理都是同一題，畫面不會跳動。
 */
export function pickGuidingQuestion(conversationId: string, questions: string[]): string | null {
  if (questions.length === 0) return null
  let h = 2166136261
  for (let i = 0; i < conversationId.length; i++) {
    h ^= conversationId.charCodeAt(i)
    h = Math.imul(h, 16777619)
  }
  return questions[(h >>> 0) % questions.length]
}
