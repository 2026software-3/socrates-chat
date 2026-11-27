export type StageNumber = 1 | 2 | 3

/**
 * 對話回合數換算追問階段（DESIGN.md Dialogue）：
 * 1–5 回合（含尚未發言的 <= 0）為階段 1（釐清）、6–12 為階段 2（論證）、13 以上為階段 3（挑戰）。
 */
export function stageForRound(round: number): StageNumber {
  if (round === Number.POSITIVE_INFINITY) return 3
  if (!Number.isFinite(round) || round <= 5) return 1
  if (round <= 12) return 2
  return 3
}

/**
 * 分段比例長條的各段百分比（0–100）。
 * 非正數或非有限值視為 0；總和為 0 時全回傳 0，避免產生 NaN。
 */
export function stackBarProportions(values: readonly number[]): number[] {
  const safe = values.map((v) => (Number.isFinite(v) && v > 0 ? v : 0))
  const total = safe.reduce((sum, v) => sum + v, 0)
  if (total <= 0) return safe.map(() => 0)
  return safe.map((v) => (v / total) * 100)
}

/** 單一比例換算為 0–100 百分比（例如參與人數 ÷ 教室學生總數）。 */
export function ratioPercent(part: number, total: number): number {
  if (!Number.isFinite(part) || !Number.isFinite(total) || total <= 0 || part <= 0) return 0
  if (part >= total) return 100
  return (part / total) * 100
}

const ROMAN_PAIRS: ReadonlyArray<readonly [number, string]> = [
  [1000, 'M'],
  [900, 'CM'],
  [500, 'D'],
  [400, 'CD'],
  [100, 'C'],
  [90, 'XC'],
  [50, 'L'],
  [40, 'XL'],
  [10, 'X'],
  [9, 'IX'],
  [5, 'V'],
  [4, 'IV'],
  [1, 'I'],
]

/** 將正整數（1–3999）轉為大寫羅馬數字；超出範圍時回傳字串形式。 */
export function toRoman(value: number): string {
  const n = Math.trunc(value)
  if (!Number.isFinite(n) || n < 1 || n > 3999) return String(value)
  let remaining = n
  let out = ''
  for (const [unit, symbol] of ROMAN_PAIRS) {
    while (remaining >= unit) {
      out += symbol
      remaining -= unit
    }
  }
  return out
}
