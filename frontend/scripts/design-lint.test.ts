import { describe, expect, it } from 'vitest'
import { checkAllowlist, lintSource, type Violation } from './design-lint.ts'

// 測試資料都是合成的程式片段，不是真實頁面。
const lint = (source: string, file = 'src/features/demo/demo.tsx') => lintSource(file, source)
const rules = (violations: Violation[]) => violations.map((v) => v.rule)

describe('原始色碼', () => {
  it('擋下字串裡的 #hex，並指出對應的 token', () => {
    const [v, ...rest] = lint(`export const A = () => <div className="text-[#A9442B]" />`)
    expect(rest).toEqual([])
    expect(v?.rule).toBe('raw-color')
    expect(v?.message).toContain('#A9442B')
    expect(v?.message).toContain('`text-chalk`')
  })

  it('不在 token 裡的色碼也擋，並說明改用 token utility', () => {
    const [v] = lint(`const style = { color: '#123456' }`)
    expect(v?.rule).toBe('raw-color')
    expect(v?.message).toContain('`text-ink-soft`')
  })

  it('擋下 rgb()、rgba()、hsl()、oklch()', () => {
    const source = `
      const a = 'rgb(31 26 22)'
      const b = \`bg-[rgba(0,0,0,0.5)]\`
      const c = { color: 'hsl(10 20% 30%)' }
      const d = 'oklch(0.5 0.1 30)'
    `
    expect(rules(lint(source))).toEqual(['raw-color', 'raw-color', 'raw-color', 'raw-color'])
  })

  it('註解、Issue 編號與 JSX 內文裡的 # 不算色碼', () => {
    const source = `
      // 見 #413 與 #abc
      /* rgb(0 0 0) 只是說明 */
      export const A = () => <p>第 #123 號</p>
    `
    expect(lint(source)).toEqual([])
  })

  it('回報正確的行號', () => {
    const [v] = lint(`const a = 1\nconst b = 'bg-[#fff]'\n`)
    expect(v?.line).toBe(2)
  })
})

describe('任意字級', () => {
  it('擋下 text-[15px]，並指出對應的文字樣式', () => {
    const [v] = lint(`<p className="text-[15px] text-ink" />`)
    expect(v?.rule).toBe('arbitrary-font-size')
    expect(v?.message).toContain('`type-body`')
  })

  it('rem 與沒有對應樣式的字級也擋', () => {
    expect(rules(lint(`const a = 'text-[0.8rem]'; const b = 'text-[17.5px]'`))).toEqual([
      'arbitrary-font-size',
      'arbitrary-font-size',
    ])
  })

  it('text-ink、text-sm 這類 token 與 Tailwind 字級不擋', () => {
    expect(lint(`const a = 'text-ink-soft text-sm type-note'`)).toEqual([])
  })
})

describe('用透明度表示停用', () => {
  it('擋下 disabled:opacity-50 與 aria-disabled、data-[disabled] 的變體', () => {
    const source = `
      const a = 'disabled:opacity-50'
      const b = 'aria-disabled:opacity-60'
      const c = 'data-[disabled]:opacity-40'
    `
    const found = lint(source)
    expect(rules(found)).toEqual(['opacity-disabled', 'opacity-disabled', 'opacity-disabled'])
    expect(found[0]?.message).toContain('disabled:text-ink-soft')
  })

  it('擋下依 disabled 條件加上的 opacity-*', () => {
    const source = `
      const a = (disabled: boolean) => (disabled ? 'opacity-50' : '')
      const b = (isDisabled: boolean) => isDisabled && 'opacity-40'
    `
    expect(rules(lint(source))).toEqual(['opacity-disabled', 'opacity-disabled'])
  })

  it('只看停用那一側的分支，啟用時的 opacity 不擋', () => {
    const source = `
      const a = (disabled: boolean) => (disabled ? '' : 'opacity-100')
      const b = (status: string) => (status !== 'disabled' ? 'opacity-100' : '')
      const c = (disabled: boolean) => !disabled && 'opacity-100'
    `
    expect(lint(source)).toEqual([])
  })

  it('條件反過來寫時，仍擋下停用那一側', () => {
    const source = `
      const a = (disabled: boolean) => (!disabled ? 'opacity-100' : 'opacity-50')
      const b = (status: string) => (status === 'disabled' ? 'opacity-50' : '')
      const c = (status: string) => (status !== 'disabled' ? '' : 'opacity-40')
    `
    const found = lint(source)
    expect(rules(found)).toEqual(['opacity-disabled', 'opacity-disabled', 'opacity-disabled'])
    expect(found.map((v) => v.message.match(/opacity-\d+/)?.[0])).toEqual(['opacity-50', 'opacity-50', 'opacity-40'])
  })

  it('擋下 CVA 的 disabled variant（true 那一側），false 那一側不擋', () => {
    const source = `
      const button = cva('px-4', {
        variants: {
          size: { sm: 'h-8' },
          disabled: { true: 'opacity-50', false: 'opacity-100' },
        },
      })
      const legacy = { disabled: 'opacity-60' }
    `
    const found = lint(source)
    expect(rules(found)).toEqual(['opacity-disabled', 'opacity-disabled'])
    expect(found.map((v) => v.line)).toEqual([5, 8])
  })

  it('和停用無關的 opacity 不擋', () => {
    expect(lint(`const a = (open: boolean) => (open ? 'opacity-100' : 'opacity-0')`)).toEqual([])
  })

  it('擋下 Tailwind 任意值 disabled:opacity-[...]（以及 aria-disabled、data-[disabled] 與停用分支）', () => {
    const source = `
      const a = 'disabled:opacity-[0.5]'
      const b = 'disabled:opacity-[.4]'
      const c = 'disabled:opacity-[50%]'
      const d = 'aria-disabled:opacity-[0.6]'
      const e = 'data-[disabled]:opacity-[0.4]'
      const f = (disabled: boolean) => (disabled ? 'opacity-[0.5]' : '')
    `
    const found = lint(source)
    expect(rules(found)).toEqual([
      'opacity-disabled',
      'opacity-disabled',
      'opacity-disabled',
      'opacity-disabled',
      'opacity-disabled',
      'opacity-disabled',
    ])
  })

  it('完全不透明的 disabled:opacity-[1]／disabled:opacity-100 不擋', () => {
    const source = `
      const a = 'disabled:opacity-100 disabled:opacity-[1] disabled:opacity-[100%]'
    `
    expect(lint(source)).toEqual([])
  })
})

describe('旋轉載入圈', () => {
  it('擋下 animate-spin，並要求改用 Skeleton', () => {
    const [v] = lint(`<span className="animate-spin" />`)
    expect(v?.rule).toBe('spinner')
    expect(v?.message).toContain('Skeleton')
  })

  it('擋下 lucide 的載入圈圖示與 Spinner 元件', () => {
    const source = `
      import { Loader2, LoaderCircle, Check } from 'lucide-react'
      import { Spinner } from '@/components/ui/spinner'
    `
    expect(rules(lint(source))).toEqual(['spinner', 'spinner', 'spinner'])
  })
})

describe('CSS 檔', () => {
  it('擋下 CSS 裡的色碼，但略過註解', () => {
    const source = `/* #fff 只是說明 */\n.a { color: #fff; }\n.b { background: rgb(0 0 0); }\n`
    expect(rules(lintSource('src/features/demo/demo.css', source))).toEqual(['raw-color', 'raw-color'])
  })

  it('擋下 CSS :disabled（含屬性選取器與巢狀）裡的 opacity 與 @apply opacity-*', () => {
    const source = `
      .a:disabled { opacity: 0.5; }
      .b:disabled { opacity: .4 }
      .c[disabled] { opacity: 50%; }
      .d[aria-disabled="true"] { opacity: 0.6; }
      .e[data-disabled] { @apply opacity-50; }
      .f { &:disabled { opacity: 0.4; } }
      .g:disabled { @apply opacity-[0.5]; }
    `
    const found = lintSource('src/components/vedute/demo.css', source)
    expect(rules(found)).toEqual([
      'opacity-disabled',
      'opacity-disabled',
      'opacity-disabled',
      'opacity-disabled',
      'opacity-disabled',
      'opacity-disabled',
      'opacity-disabled',
    ])
    expect(found[0]?.line).toBe(2)
  })

  it('CSS :not(:disabled)、opacity: 1／opacity-100 與 transition: opacity 不誤判', () => {
    const source = `
      .a:not(:disabled) { opacity: 0.8; }
      .b:disabled, .c { @apply opacity-100; }
      .d:disabled { opacity: 1; transition: opacity 150ms; }
      .e:disabled { opacity: 100%; }
      .f[aria-disabled="false"] { opacity: 0.5; }
      @keyframes pulse { 50% { opacity: 0.55; } }
    `
    expect(lintSource('src/components/vedute/demo.css', source)).toEqual([])
  })
})

describe('允許清單只能縮小', () => {
  const v = (file: string, rule: Violation['rule']): Violation => ({ file, rule, line: 1, column: 1, message: 'x' })

  it('數量在清單之內時通過', () => {
    const result = checkAllowlist([v('a.tsx', 'raw-color')], { 'a.tsx': { 'raw-color': 1 } })
    expect(result).toEqual({ errors: [], shrinkable: [] })
  })

  it('新增的違規（不在清單或超過數量）都回報', () => {
    const result = checkAllowlist([v('a.tsx', 'raw-color'), v('a.tsx', 'raw-color'), v('b.tsx', 'spinner')], {
      'a.tsx': { 'raw-color': 1 },
    })
    expect(result.errors.map((e) => `${e.file}:${e.rule}`)).toEqual(['a.tsx:raw-color', 'a.tsx:raw-color', 'b.tsx:spinner'])
    expect(result.shrinkable).toEqual([])
  })

  it('修掉違規後要求把清單數字降下來', () => {
    const result = checkAllowlist([], { 'a.tsx': { 'raw-color': 2 } })
    expect(result.errors).toEqual([])
    expect(result.shrinkable).toEqual([{ file: 'a.tsx', rule: 'raw-color', allowed: 2, actual: 0 }])
  })
})
