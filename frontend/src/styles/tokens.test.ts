import { readFileSync } from 'node:fs'
import path from 'node:path'

// Vedute 設計 token（F-41.1）：數值來自 Figma [00] Guideline 的 Vedute 變數與文字樣式，規則見 DESIGN.md。
const read = (file: string) => readFileSync(path.resolve(import.meta.dirname, file), 'utf8')
const tokens = read('./tokens.css')
const index = read('../index.css')

/** 取出 `--name: value;` 的值（去掉前後空白），找不到回傳 undefined。 */
function declared(css: string, name: string) {
  const match = new RegExp(`--${name}\\s*:\\s*([^;]+);`).exec(css)
  return match?.[1]?.trim()
}

/** 取出 `@utility name { ... }` 的內容。 */
function utility(css: string, name: string) {
  const match = new RegExp(`@utility\\s+${name}\\s*\\{([^}]*)\\}`).exec(css)
  return match?.[1]
}

const FIGMA_COLORS: Record<string, string> = {
  paper: '#e8e1cf',
  'paper-deep': '#ddd4be',
  surface: '#ffffff',
  ink: '#1f1a16',
  'ink-soft': '#4a4038',
  'ink-faint': 'rgb(31 26 22 / 0.28)',
  placeholder: '#665c4e',
  error: '#8a1c12',
  chalk: '#a9442b',
  'chalk-deep': '#8a3520',
  'chalk-wash': '#f1ded6',
  aerugo: '#3b6455',
  'aerugo-wash': '#dce6e0',
  ochra: '#74561b',
  'ochra-wash': '#f0e4c4',
  'ochra-graphic': '#b0862f',
  caeruleum: '#34557a',
  'caeruleum-wash': '#dce3ec',
}

describe('Vedute 色彩 token', () => {
  it.each(Object.entries(FIGMA_COLORS))('--color-%s 與 Figma 一致', (name, value) => {
    expect(declared(tokens, `color-${name}`)?.toLowerCase()).toBe(value)
  })

  it('token 寫在 Tailwind @theme 裡，才會產生 bg-paper、text-ink-soft 等 utility', () => {
    expect(tokens).toMatch(/@theme\s*\{/)
  })
})

describe('shadcn 變數對應到 Vedute token', () => {
  const SHADCN: Record<string, string> = {
    background: 'surface',
    foreground: 'ink',
    card: 'surface',
    'card-foreground': 'ink',
    popover: 'surface',
    'popover-foreground': 'ink',
    primary: 'ink',
    'primary-foreground': 'paper',
    'primary-hover': 'chalk',
    secondary: 'paper-deep',
    'secondary-foreground': 'ink',
    muted: 'paper',
    'muted-foreground': 'ink-soft',
    accent: 'paper',
    'accent-foreground': 'ink',
    destructive: 'error',
    border: 'ink-faint',
    input: 'ink-soft',
    ring: 'chalk',
    subtle: 'paper',
    'outline-border': 'ink-faint',
    sidebar: 'paper',
    'sidebar-foreground': 'ink',
  }

  it.each(Object.entries(SHADCN))('--%s 指向 --color-%s', (name, token) => {
    expect(declared(index, name)).toBe(`var(--color-${token})`)
  })

  it('不再有任意色碼或深色模式的另一套數值', () => {
    const root = /:root\s*\{([^}]*)\}/.exec(index)?.[1] ?? ''
    expect(root).not.toMatch(/#[0-9a-f]{3,8}\b|oklch\(/i)
    expect(index).not.toMatch(/^\.dark\s*\{/m)
  })
})

describe('字型', () => {
  it('改用 Noto Serif TC 與 IM FELL English，移除 Inter 與 Noto Sans', () => {
    expect(index).not.toMatch(/inter|noto-sans/i)
    expect(index).toMatch(/@fontsource-variable\/noto-serif-tc/)
    expect(index).toMatch(/@fontsource\/im-fell-english-sc/)
    expect(index).toMatch(/@fontsource\/im-fell-english\//)
    expect(declared(tokens, 'font-sans')).toMatch(/^'Noto Serif TC Variable'/)
    expect(declared(tokens, 'font-latin')).toMatch(/^'IM FELL English SC'/)
    expect(declared(tokens, 'font-chalk')).toMatch(/^'IM FELL English'/)
  })
})

describe('文字樣式（Figma Title/Text/Latin）', () => {
  const STYLES: Array<[string, string, string, string]> = [
    // utility、字級、行高、字重
    ['type-h1', '28px', '36px', '900'],
    ['type-h2', '22px', '31px', '700'],
    ['type-h3', '18px', '27px', '700'],
    ['type-body', '15px', '26px', '400'],
    ['type-body-strong', '15px', '26px', '700'],
    ['type-note', '13px', '21px', '400'],
    ['type-latin-label', '17px', '22px', '400'],
    ['type-latin-numeral', '20px', '24px', '400'],
    ['type-latin-small', '12px', '1.2', '400'],
    ['type-chalk-note', '15px', '20px', '400'],
  ]

  it.each(STYLES)('%s 是 %s／%s、字重 %s', (name, size, lineHeight, weight) => {
    const body = utility(tokens, name)
    expect(body).toBeDefined()
    expect(body).toMatch(new RegExp(`font-size:\\s*${size}`))
    expect(body).toMatch(new RegExp(`line-height:\\s*${lineHeight.replace('.', '\\.')}`))
    expect(body).toMatch(new RegExp(`font-weight:\\s*${weight}`))
  })

  it('type-display 在手機縮小，最大 40px', () => {
    expect(utility(tokens, 'type-display')).toMatch(/font-size:\s*clamp\(28px,[^)]*,\s*40px\)/)
  })
})

describe('形狀與深度', () => {
  it('圓角只有 0、2px 與圓形', () => {
    for (const size of ['xs', 'sm', 'md', 'lg', 'xl', '2xl', '3xl', '4xl']) {
      expect(declared(tokens, `radius-${size}`)).toBe('2px')
    }
  })

  it('不使用投影', () => {
    for (const size of ['2xs', 'xs', 'sm', 'md', 'lg', 'xl', '2xl']) {
      expect(declared(tokens, `shadow-${size}`)).toBe('0 0 #0000')
    }
  })
})
