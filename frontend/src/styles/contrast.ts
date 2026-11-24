/** 把 #rgb／#rrggbb 轉成 0–1 的 sRGB 三個分量；其他格式（含透明度）直接報錯，避免默默算錯。 */
function channels(hex: string): [number, number, number] {
  const match = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(hex.trim())
  if (!match?.[1]) throw new Error(`contrastRatio 只接受不透明的 #hex 顏色，收到 ${hex}`)
  const digits = match[1].length === 3 ? [...match[1]].map((d) => d + d).join('') : match[1]
  return [0, 2, 4].map((i) => parseInt(digits.slice(i, i + 2), 16) / 255) as [number, number, number]
}

/** WCAG 2.x 的相對亮度。 */
function luminance(hex: string) {
  const [r, g, b] = channels(hex).map((c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4)) as [
    number,
    number,
    number,
  ]
  return 0.2126 * r + 0.7152 * g + 0.0722 * b
}

/** WCAG 2.x 對比（1–21），與前景、背景的順序無關。 */
export function contrastRatio(foreground: string, background: string) {
  const a = luminance(foreground)
  const b = luminance(background)
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05)
}
