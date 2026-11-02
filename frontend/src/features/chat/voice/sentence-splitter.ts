/**
 * 把生成中的回覆切成可以唸的句子（S-05.2）。斷句交給瀏覽器的 `Intl.Segmenter`，
 * 中文標點、小數點、結尾引號都由它處理，不需要自己維護規則。
 *
 * 最後一段可能還沒講完，要等下一段出現或回覆結束才交出去。
 * 瀏覽器不支援時等回覆結束才唸，行為與不切句相同。
 */

/** 傳入目前累積的全文，回傳這次新完成的句子；`final` 表示回覆已結束。 */
export type SentenceSplitter = (text: string, final: boolean) => string[]

export function createSentenceSplitter(lang: string): SentenceSplitter {
  const segmenter = typeof Intl.Segmenter === 'function' ? new Intl.Segmenter(lang, { granularity: 'sentence' }) : undefined
  let consumed = 0
  return (text, final) => {
    const rest = text.slice(consumed)
    const segments = segmenter ? Array.from(segmenter.segment(rest), (s) => s.segment) : [rest]
    const ready = final ? segments : segments.slice(0, -1)
    consumed += ready.reduce((sum, s) => sum + s.length, 0)
    return ready.map((s) => s.trim()).filter(Boolean)
  }
}
