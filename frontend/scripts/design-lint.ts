/**
 * 設計規則 lint（F-41.6）：把 DESIGN.md 的規則變成確定性檢查。
 * 每則錯誤訊息都寫明根因與正確寫法，讓人與 agent 照著改就好。
 *
 * 檢查範圍：src/features/、src/components/vedute/ 的 .ts、.tsx、.css（測試檔除外）。
 * 只看字串與 class，不看註解與 JSX 內文，所以「#413」這類 Issue 編號不會誤判。
 *
 * 既有違規記在 design-lint-allowlist.json（檔案 → 規則 → 數量），只能縮小：
 * 數量超過清單就失敗；修掉之後數量低於清單也失敗，提醒把清單降下來。
 *
 * 用法：npm run lint:design（npm run lint 會一起跑）
 */
import { readdirSync, readFileSync } from 'node:fs'
import path from 'node:path'
import ts from 'typescript'

export type Rule = 'raw-color' | 'arbitrary-font-size' | 'opacity-disabled' | 'spinner'

export interface Violation {
  file: string
  rule: Rule
  line: number
  column: number
  message: string
}

export type Allowlist = Record<string, Partial<Record<Rule, number>>>

export interface Shrinkable {
  file: string
  rule: Rule
  allowed: number
  actual: number
}

const FRONTEND = path.resolve(import.meta.dirname, '..')
const SCANNED_DIRS = ['src/features', 'src/components/vedute']
const ALLOWLIST_FILE = 'design-lint-allowlist.json'

// ---- token 對照：讓訊息直接說「這個色碼就是 chalk」 ----

function normalizeHex(hex: string) {
  const digits = hex.slice(1).toLowerCase()
  return digits.length === 3 || digits.length === 4 ? [...digits].map((d) => d + d).join('') : digits
}

function loadColorTokens() {
  const css = readFileSync(path.join(FRONTEND, 'src/styles/tokens.css'), 'utf8')
  const byHex = new Map<string, string>()
  for (const [, name, value] of css.matchAll(/--color-([\w-]+)\s*:\s*(#[0-9a-fA-F]+)\s*;/g)) {
    if (name && value) byHex.set(normalizeHex(value), name)
  }
  return byHex
}

const COLOR_TOKENS = loadColorTokens()

/** DESIGN.md Typography 的字級 → tokens.css 的文字樣式 */
const TYPE_BY_PX: Record<number, string> = {
  28: 'type-h1',
  22: 'type-h2',
  18: 'type-h3',
  15: 'type-body',
  13: 'type-note',
  12: 'type-latin-small',
  17: 'type-latin-label',
  19: 'type-latin-inscription',
  20: 'type-latin-numeral',
}

// ---- 訊息 ----

function rawColorMessage(found: string) {
  const token = found.startsWith('#') ? COLOR_TOKENS.get(normalizeHex(found)) : undefined
  const fix = token
    ? `這個色碼就是 token \`${token}\`，改用 \`text-${token}\`、\`bg-${token}\` 或 \`border-${token}\`。`
    : '改用 tokens.css 的 token utility，例如 `text-ink-soft`、`bg-paper`、`border-ink`、`bg-caeruleum-wash`；需要的顏色不在 token 裡時，先更新 DESIGN.md 與 tokens.css，不要在頁面自己寫。'
  return `原始色碼 \`${found}\`：Vedute 的顏色只能來自 tokens.css，散在頁面裡的色碼改版時改不到，也沒有對比檢查。${fix}`
}

function fontSizeMessage(found: string, size: string) {
  const px = size.endsWith('rem') ? parseFloat(size) * 16 : parseFloat(size)
  const style = TYPE_BY_PX[px]
  const fix = style
    ? `${px}px 對應的文字樣式是 \`${style}\`，改用它（字級、行高、字重一起決定）。`
    : '改用 DESIGN.md Typography 的文字樣式：`type-h1`／`type-h2`／`type-h3`／`type-body`／`type-body-strong`／`type-note`，拉丁刻字用 `type-latin-*`；樣式不夠用時先更新 DESIGN.md 與 tokens.css。'
  return `任意字級 \`${found}\`：字級只能用 DESIGN.md 定義的文字樣式，否則同一層級的字會各寫各的。${fix}`
}

function opacityMessage(found: string) {
  return `用透明度表示停用（\`${found}\`）：整個元素變透明後文字對比會掉到 3:1 以下（DESIGN.md「停用不用透明度」）。改成透明底、1px 淡墨框、淡墨字，例如 \`disabled:bg-transparent disabled:border-ink-soft disabled:text-ink-soft disabled:cursor-not-allowed\`；按鈕直接用 components/vedute 的按鈕，它已內建停用樣式。`
}

function spinnerMessage(found: string) {
  return `旋轉載入圈（\`${found}\`）：DESIGN.md「載入中（Skeleton）」規定載入一律用 skeleton，載入完成時版面才不會跳動。改用 components/vedute 的 \`Skeleton\` 或對應版面的組合（例如 \`DirectoryRowSkeleton\`），並在載入區域設 \`aria-busy="true"\`；按鈕送出中只停用並改文字（例如「儲存中…」），等待 AI 回覆用「思考中」文字。`
}

// ---- 字串內容的規則 ----

interface Hit {
  rule: Rule
  index: number
  message: string
}

const HEX = /(?<![\w&])#(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{3,4})(?![\w-])/g
const COLOR_FN = /\b(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch)\(/g
const FONT_SIZE = /(?<![\w-])text-\[(?:length:)?(\d*\.?\d+(?:px|rem|em))\]/g
const DISABLED_OPACITY = /(?<!\S)\S*disabled\S*:opacity-\d+/g
const ANY_OPACITY = /(?<![\w-])opacity-\d+/g
const ANIMATE_SPIN = /(?<![\w-])animate-spin(?![\w-])/g

function scanText(text: string, { skipHex = false } = {}): Hit[] {
  const hits: Hit[] = []
  const add = (re: RegExp, rule: Rule, message: (m: RegExpMatchArray) => string) => {
    for (const m of text.matchAll(re)) hits.push({ rule, index: m.index ?? 0, message: message(m) })
  }
  if (!skipHex) add(HEX, 'raw-color', (m) => rawColorMessage(m[0]))
  add(COLOR_FN, 'raw-color', (m) => rawColorMessage(`${m[0]}…)`))
  add(FONT_SIZE, 'arbitrary-font-size', (m) => fontSizeMessage(m[0], m[1] ?? ''))
  add(DISABLED_OPACITY, 'opacity-disabled', (m) => opacityMessage(m[0]))
  add(ANIMATE_SPIN, 'spinner', (m) => spinnerMessage(m[0]))
  return hits.sort((a, b) => a.index - b.index)
}

// ---- TS／TSX ----

const SPINNER_NAME = /^(?:Loader|Loader2|LoaderCircle|LoaderPinwheel|Loader2Icon|LoaderCircleIcon)$|Spinner/

function mentionsDisabled(node: ts.Node, sf: ts.SourceFile) {
  return /disabled/i.test(node.getText(sf))
}

/**
 * 條件成立時是不是「停用」：`disabled`、`status === 'disabled'` 是 true；
 * `!disabled`、`status !== 'disabled'` 是 false；和停用無關回傳 undefined。
 */
function disabledWhenTrue(cond: ts.Expression, sf: ts.SourceFile): boolean | undefined {
  if (ts.isParenthesizedExpression(cond)) return disabledWhenTrue(cond.expression, sf)
  if (ts.isPrefixUnaryExpression(cond) && cond.operator === ts.SyntaxKind.ExclamationToken) {
    const inner = disabledWhenTrue(cond.operand, sf)
    return inner === undefined ? undefined : !inner
  }
  if (ts.isBinaryExpression(cond)) {
    const op = cond.operatorToken.kind
    if (op === ts.SyntaxKind.ExclamationEqualsEqualsToken || op === ts.SyntaxKind.ExclamationEqualsToken)
      return mentionsDisabled(cond, sf) ? false : undefined
    if (op === ts.SyntaxKind.AmpersandAmpersandToken || op === ts.SyntaxKind.BarBarToken)
      return disabledWhenTrue(cond.left, sf) ?? disabledWhenTrue(cond.right, sf)
  }
  return mentionsDisabled(cond, sf) ? true : undefined
}

/**
 * 這個字串是否只在「停用」時才加上：`disabled ? 'opacity-50' : ''`、`isDisabled && 'opacity-40'`、
 * `{ disabled: 'opacity-50' }`、CVA 的 `variants: { disabled: { true: 'opacity-50' } }`。
 * 只看停用那一側，`disabled ? '' : 'opacity-100'` 這類啟用時的透明度不算。
 */
function underDisabledCondition(node: ts.Node, sf: ts.SourceFile) {
  let child = node
  let innerKey: string | undefined // 最靠近字串的屬性名稱，用來分辨 CVA 的 true／false
  for (let parent = node.parent; parent; child = parent, parent = parent.parent) {
    if (ts.isConditionalExpression(parent)) {
      if (child === parent.condition) return false
      const polarity = disabledWhenTrue(parent.condition, sf)
      if (polarity === undefined) return false
      return child === parent.whenTrue ? polarity : !polarity
    }
    if (ts.isBinaryExpression(parent)) {
      if (parent.operatorToken.kind !== ts.SyntaxKind.AmpersandAmpersandToken || child !== parent.right) return false
      return disabledWhenTrue(parent.left, sf) === true
    }
    if (ts.isPropertyAssignment(parent)) {
      if (child !== parent.initializer) return false
      if (mentionsDisabled(parent.name, sf)) return innerKey !== 'false'
      innerKey ??= parent.name.getText(sf)
      continue
    }
    if (ts.isParenthesizedExpression(parent) || ts.isObjectLiteralExpression(parent)) continue
    return false
  }
  return false
}

function lintTs(file: string, source: string): Violation[] {
  const kind = file.endsWith('.tsx') ? ts.ScriptKind.TSX : ts.ScriptKind.TS
  const sf = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, kind)
  const out: Violation[] = []
  const report = (pos: number, rule: Rule, message: string) => {
    const { line, character } = sf.getLineAndCharacterOfPosition(pos)
    out.push({ file, rule, line: line + 1, column: character + 1, message })
  }

  const visit = (node: ts.Node) => {
    if (
      ts.isStringLiteral(node) ||
      ts.isNoSubstitutionTemplateLiteral(node) ||
      ts.isTemplateHead(node) ||
      ts.isTemplateMiddle(node) ||
      ts.isTemplateTail(node)
    ) {
      const isModulePath = ts.isImportDeclaration(node.parent) || ts.isExportDeclaration(node.parent)
      if (!isModulePath) {
        const start = node.getStart(sf) + 1
        for (const hit of scanText(node.text)) report(start + hit.index, hit.rule, hit.message)
        if (underDisabledCondition(node, sf)) {
          for (const m of node.text.matchAll(ANY_OPACITY)) {
            // disabled:opacity-* 已由 scanText 回報
            if (!/\S*disabled\S*:$/.test(node.text.slice(0, m.index))) {
              report(start + (m.index ?? 0), 'opacity-disabled', opacityMessage(`${m[0]}（停用時才加上）`))
            }
          }
        }
      }
    } else if (ts.isImportSpecifier(node) || ts.isImportClause(node)) {
      const name = (ts.isImportSpecifier(node) ? (node.propertyName ?? node.name) : node.name)?.text
      if (name && SPINNER_NAME.test(name)) report(node.getStart(sf), 'spinner', spinnerMessage(name))
    }
    ts.forEachChild(node, visit)
  }
  visit(sf)
  return out
}

// ---- CSS ----

function lintCss(file: string, source: string): Violation[] {
  // 註解換成等長空白，位置不變
  const code = source.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, ' '))
  return scanText(code).map((hit) => {
    const before = code.slice(0, hit.index)
    const line = before.split('\n').length
    const column = hit.index - before.lastIndexOf('\n')
    return { file, rule: hit.rule, line, column, message: hit.message }
  })
}

export function lintSource(file: string, source: string): Violation[] {
  return file.endsWith('.css') ? lintCss(file, source) : lintTs(file, source)
}

// ---- 允許清單 ----

export function checkAllowlist(violations: Violation[], allowlist: Allowlist) {
  const groups = new Map<string, Violation[]>()
  for (const v of violations) {
    const key = `${v.file}\0${v.rule}`
    groups.set(key, [...(groups.get(key) ?? []), v])
  }
  const errors: Violation[] = []
  for (const group of groups.values()) {
    const first = group[0]
    if (first && group.length > (allowlist[first.file]?.[first.rule] ?? 0)) errors.push(...group)
  }
  const shrinkable: Shrinkable[] = []
  for (const [file, rules] of Object.entries(allowlist)) {
    for (const [rule, allowed = 0] of Object.entries(rules) as [Rule, number][]) {
      const actual = groups.get(`${file}\0${rule}`)?.length ?? 0
      if (actual < allowed) shrinkable.push({ file, rule, allowed, actual })
    }
  }
  return { errors, shrinkable }
}

// ---- CLI ----

function listFiles(dir: string): string[] {
  let entries
  try {
    entries = readdirSync(path.join(FRONTEND, dir), { withFileTypes: true })
  } catch {
    return [] // 目錄還不存在（例如 components/vedute 尚未建立）
  }
  return entries.flatMap((entry) => {
    const rel = `${dir}/${entry.name}`
    if (entry.isDirectory()) return listFiles(rel)
    if (!/\.(tsx?|css)$/.test(entry.name) || /\.test\.tsx?$/.test(entry.name)) return []
    return [rel]
  })
}

function main() {
  const files = SCANNED_DIRS.flatMap(listFiles)
  const violations = files.flatMap((file) => lintSource(file, readFileSync(path.join(FRONTEND, file), 'utf8')))
  const allowlist = JSON.parse(readFileSync(path.join(FRONTEND, ALLOWLIST_FILE), 'utf8')) as Allowlist
  const { errors, shrinkable } = checkAllowlist(violations, allowlist)

  for (const v of errors) {
    const allowed = allowlist[v.file]?.[v.rule]
    const note = allowed ? `（此檔允許 ${allowed} 處既有違規，現在超過了）` : ''
    console.error(`${v.file}:${v.line}:${v.column}  design/${v.rule}  ${v.message}${note}`)
  }
  for (const s of shrinkable) {
    console.error(
      `${ALLOWLIST_FILE}  ${s.file} 的 ${s.rule} 已從 ${s.allowed} 處減到 ${s.actual} 處，請把清單的數字改成 ${s.actual}${s.actual === 0 ? '（或刪掉這一項）' : ''}；允許清單只能縮小。`,
    )
  }
  if (errors.length > 0 || shrinkable.length > 0) {
    console.error(`\n設計規則 lint：${errors.length} 處違規、${shrinkable.length} 項允許清單需要縮小。規則見 DESIGN.md。`)
    process.exitCode = 1
  } else {
    console.log(`設計規則 lint：${files.length} 個檔案通過。`)
  }
}

if (import.meta.main) main()
