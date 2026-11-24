import { readFileSync } from 'node:fs'
import path from 'node:path'
import { contrastRatio } from './contrast'

// DESIGN.md Colors 的前景／背景組合：文字 ≥ 4.5:1（WCAG 1.4.3），元件邊界與圖形 ≥ 3:1（WCAG 1.4.11）。
// 顏色直接讀 tokens.css，token 改了而對比不夠時這裡會失敗。
const tokens = readFileSync(path.resolve(import.meta.dirname, './tokens.css'), 'utf8')

function token(name: string) {
  const value = new RegExp(`--color-${name}\\s*:\\s*([^;]+);`).exec(tokens)?.[1]?.trim()
  if (!value) throw new Error(`tokens.css 沒有 --color-${name}`)
  return value
}

const ratio = (fg: string, bg: string) => contrastRatio(token(fg), token(bg))

describe('contrastRatio', () => {
  it('黑對白是 21:1、同色是 1:1', () => {
    expect(contrastRatio('#000000', '#FFFFFF')).toBeCloseTo(21, 5)
    expect(contrastRatio('#fff', '#ffffff')).toBeCloseTo(1, 5)
  })

  it('前景與背景對調結果相同', () => {
    expect(contrastRatio('#1F1A16', '#E8E1CF')).toBeCloseTo(contrastRatio('#E8E1CF', '#1F1A16'), 10)
  })

  it('不是 #hex 時直接報錯，不默默算錯', () => {
    expect(() => contrastRatio('rgb(31 26 22 / 0.28)', '#ffffff')).toThrow(/#hex/)
  })
})

/** [前景, 背景, DESIGN.md 寫的對比（沒寫就省略）] */
type Pair = [string, string, number?]

// 文字：DESIGN.md Colors 有寫數字的組合同時核對數字，文件和 token 不一致時要一起修。
const TEXT: Pair[] = [
  ['ink', 'paper', 13.2],
  ['ink', 'paper-deep'],
  ['ink', 'surface'],
  ['ink-soft', 'paper', 7.7],
  ['ink-soft', 'paper-deep'],
  ['ink-soft', 'surface'],
  ['placeholder', 'paper', 5.0],
  ['placeholder', 'surface'],
  ['error', 'paper', 7.1],
  ['error', 'surface'],
  ['chalk', 'paper'],
  ['chalk', 'surface'],
  ['chalk-deep', 'paper', 6.17],
  ['chalk-deep', 'paper-deep', 5.45],
  ['aerugo', 'paper', 5.1],
  ['aerugo', 'surface', 6.7],
  ['aerugo', 'paper-deep', 4.5],
  ['aerugo', 'aerugo-wash'],
  ['ochra', 'paper', 5.2],
  ['ochra', 'surface', 6.8],
  ['ochra', 'paper-deep', 4.6],
  ['ochra', 'ochra-wash'],
  ['caeruleum', 'paper', 5.9],
  ['caeruleum', 'surface', 7.7],
  ['caeruleum', 'paper-deep', 5.2],
  ['caeruleum', 'caeruleum-wash'],
  ['ink', 'chalk-wash'],
  // 按鈕：Primary 墨底紙字、最關鍵行動硃砂底、領取獎勵赭金底
  ['paper', 'ink'],
  ['paper', 'chalk'],
  ['paper', 'chalk-deep'],
  ['paper', 'ochra'],
]

// 元件邊界與圖形：Quiet 與輸入框的墨框、停用的淡墨框、紅粉筆焦點框、赭金裝飾色（只放在白底的內容區）
const BOUNDARY: Pair[] = [
  ['ink', 'paper'],
  ['ink', 'paper-deep'],
  ['ink', 'surface'],
  ['ink-soft', 'paper'],
  ['ink-soft', 'surface'],
  ['chalk', 'paper'],
  ['chalk', 'paper-deep'],
  ['chalk', 'surface'],
  ['ochra-graphic', 'surface'],
]

describe('Vedute token 對比', () => {
  it.each(TEXT)('文字 %s 在 %s 上 ≥ 4.5:1', (fg, bg, documented) => {
    const actual = ratio(fg, bg)
    expect(actual).toBeGreaterThanOrEqual(4.5)
    if (documented !== undefined) expect(actual).toBeCloseTo(documented, 1)
  })

  it.each(BOUNDARY)('邊界 %s 在 %s 上 ≥ 3:1', (fg, bg) => {
    expect(ratio(fg, bg)).toBeGreaterThanOrEqual(3)
  })

  // DESIGN.md Known Issues：這兩組低於 4.5:1，深紙底與小字改用 chalk-deep、placeholder 不放深紙底。
  // 若 token 調整後已達標，請把它們移進上面的 TEXT，並更新 DESIGN.md。
  it.each([
    ['chalk', 'paper-deep'],
    ['placeholder', 'paper-deep'],
  ])('已知不足：%s 在 %s 上仍低於 4.5:1（不能當文字用）', (fg, bg) => {
    expect(ratio(fg, bg)).toBeLessThan(4.5)
  })
})
