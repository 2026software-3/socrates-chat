#!/usr/bin/env node
// 產生 docs/design/figma-map.md（Figma 節點對照表與功能地圖）。不需安裝任何套件。
//
//   node scripts/figma-map/build.mjs          # 重新產生文件
//   node scripts/figma-map/build.mjs --check  # 只檢查：文件是否最新、每個 frame 都有歸屬、列出的檔案都存在
//
// 輸入：frames/*.txt（用 dump-frames.figma.js 從 Figma 唯讀匯出）與 screens.mjs（人工維護的對照）。
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { aliases, ignore, overlaySectionIssue, pages, routesWithoutFigma } from './screens.mjs'

const here = dirname(fileURLToPath(import.meta.url))
const root = join(here, '..', '..')
const outPath = join(root, 'docs', 'design', 'figma-map.md')
const srcRoot = join(root, 'frontend', 'src')
const FILE_URL = 'https://www.figma.com/design/zW14xVQcXF8S5rQuD6YzDM/socrates-chat'
const WIDTHS = ['1440', '768', '375']
const STATUS = { redrawn: '已重畫', old: '尚未重畫（舊稿）', proposal: '僅提案' }

const nodeUrl = (id) => `${FILE_URL}?node-id=${id.replace(':', '-')}`
const nodeLink = (id) => `[${id}](${nodeUrl(id)})`
const issueLink = (n) => (n ? `#${n}` : '—')

/** 讀 dump-frames.figma.js 的輸出：`F|S <id> <w>x<h> <name>`，縮排兩格表示在上一個區塊裡。 */
function parseFrames(text) {
  const frames = []
  const sections = []
  let section = null
  for (const raw of text.split('\n')) {
    if (!raw.trim() || raw.startsWith('#')) continue
    const m = raw.match(/^( *)([FS]) (\S+) (\d+)x(\d+) (.+)$/)
    if (!m) throw new Error(`看不懂的行：${raw}`)
    const [, indent, kind, id, w, h, name] = m
    if (kind === 'S') {
      section = { id, name }
      sections.push(section)
    } else {
      frames.push({ id, w: Number(w), h: Number(h), name, section: indent ? section : null })
    }
    if (!indent && kind === 'F') section = null
  }
  return { frames, sections }
}

/** frame 寬度 → 欄位：桌機 1440、平板 768（含 580 寬的平板對話框）、手機 375（含 343 寬的手機對話框）。 */
const widthOf = (w) => (w >= 1024 ? '1440' : w >= 500 ? '768' : '375')

/** 去掉名稱裡的寬度標記，讓同一狀態的三種寬度對到同一列。 */
export function normalize(name) {
  let s = name
  s = s.replace(/^\[02\] RWD｜(Tablet|Mobile)｜/, '')
  s = s.replace(/^(桌機|平板|手機)｜/, '')
  s = s.replace(/^Entry · [DTM](\d) /, '$1 ')
  s = s.replace(/^([SRE]\d+(?:-\d)?)-[tm](?![A-Za-z0-9])/, '$1')
  s = s.replace(/^([SRE]\d+)｜(桌面|平板|手機)・/, '$1 · ')
  s = s.replace(/^([SRE]\d+(?:-\d)?)\.?\s*/, '$1 ')
  s = s.replace(/（(桌機|平板|手機)）$/, '')
  s = s.replace(/ · (平板|手機)$/, '')
  s = s.replace(/\s*·\s*/g, ' · ')
  s = s.replace(/\s+/g, ' ').trim()
  return aliases[s] ?? s
}

function statusOf(screen, state) {
  if (screen.allProposal || screen.proposal?.some((r) => r.test(state))) return 'proposal'
  if (screen.old?.some((r) => r.test(state))) return 'old'
  return 'redrawn'
}

function summarize(statuses) {
  const set = new Set(statuses)
  if (set.size === 1) return STATUS[statuses[0]]
  const parts = []
  if (set.has('redrawn')) parts.push('已重畫')
  if (set.has('old')) parts.push('部分狀態尚未重畫')
  if (set.has('proposal')) parts.push('含提案')
  return parts.join('，')
}

const code = (s) => (s ? `\`${s}\`` : '')
const routeCell = (s) => s.allProposal && !s.route ? "—" :
  s.route ? `${s.route.includes('`') || /[（、]/.test(s.route) ? s.route.replace(/(\/[\w/:.-]*)/g, '`$1`') : code(s.route)}${s.routeNote ? `（${s.routeNote}）` : ''}` : `尚無路由${s.routeNote ? `（${s.routeNote}）` : ''}`
const testsCell = (s) => (s.tests.length ? s.tests.map((t) => code(t.replace(/^features\//, ''))).join('<br>') : '—')

const problems = []
const lines = []
const out = (...l) => lines.push(...l)
const notRedrawn = []
const unclear = []

out(
  '# Figma 節點對照表與功能地圖',
  '',
  '> 本文件由 `node scripts/figma-map/build.mjs` 產生，**不要手改**；要改對照就改 `scripts/figma-map/screens.mjs`，Figma 有新畫面就重新匯出 `scripts/figma-map/frames/`（見 [`scripts/figma-map/README.md`](../../scripts/figma-map/README.md)）。',
  '',
  `把 Figma [socrates-chat](${FILE_URL}) [01]–[05] 的每個畫面對到路由、功能目錄、既有測試與負責的 F-41 Issue，讓實作時直接找到節點，不用猜。設計依據的優先順序仍是 \`DESIGN.md\` → Figma → HTML 原型（PR #653）。`,
  '',
  '## 怎麼讀',
  '',
  '- 每一頁先有**畫面總表**，再有**各畫面的 frame**：一列是一個狀態，三欄是桌機 1440、平板 768、手機 375 的節點；「—」表示該寬度沒畫。',
  '- 路由與功能目錄相對 `frontend/src`；路由來自 `app/routes.tsx` 與 `features/*/index.tsx`、`features/classrooms/routes.tsx`。教師與管理者共用的頁面同時列 `/teacher` 與 `/admin`。',
  '- 狀態：**已重畫**＝Vedute 定稿；**尚未重畫（舊稿）**＝PR #653「明確不包含」的舊畫面，只參考內容、不照版面；**僅提案**＝待需求方確認，不是規範。',
  '- 「尚無路由」＝前端還沒有這個頁面。',
  '',
  '## Figma 沒畫的狀態怎麼做',
  '',
  '- **載入中一律用 skeleton**，不論 Figma 有沒有畫、畫成什麼樣子（載入圈、點點都不用）。對話頁 AI 回覆前的「正在想下一個問題」掃光是例外，見 #683 與 `DESIGN.md` Dialogue。',
  '- 讀取失敗用 Alert（可重試）；空資料用該頁的空狀態文字，版面沿用已重畫的同頁畫面。',
  '- 只有桌機稿、缺平板或手機的狀態，依同一畫面已重畫的平板／手機版面與 `DESIGN.md` 的斷點規則延伸。',
  '',
)

for (const page of pages) {
  const { frames, sections } = parseFrames(readFileSync(join(here, 'frames', page.file), 'utf8'))
  const byScreen = new Map(page.screens.map((s) => [s, new Map()]))
  for (const f of frames) {
    if (ignore.some((r) => r.test(f.name))) continue
    const state = normalize(f.name)
    const screen = page.screens.find((s) => s.match.test(state))
    if (!screen) {
      problems.push(`${page.title}：frame ${f.id}「${f.name}」（正規化為「${state}」）沒有對到任何畫面`)
      continue
    }
    const states = byScreen.get(screen)
    const row = states.get(state) ?? {}
    const col = widthOf(f.w)
    if (row[col]) problems.push(`${page.title}：「${state}」的 ${col} 有兩個 frame（${row[col]}、${f.id}）`)
    row[col] = f.id
    states.set(state, row)
  }

  out(`## ${page.title}`, '', `Figma 頁面：[${page.nodeId}](${nodeUrl(page.nodeId)})${page.sectionNote ? `。${page.sectionNote}` : ''}`, '')
  out('| 畫面 | 路由 | 功能目錄 | 既有測試 | Issue | 狀態 |', '| --- | --- | --- | --- | --- | --- |')
  for (const s of page.screens) {
    const states = byScreen.get(s)
    if (states.size === 0) problems.push(`${page.title}：畫面「${s.name}」沒有任何 frame`)
    const statuses = [...states.keys()].map((st) => statusOf(s, st))
    out(`| ${s.name} | ${routeCell(s)} | ${s.feature ? code(s.feature.replace(/^features\//, '')) : '—'} | ${testsCell(s)} | ${issueLink(s.issue)}${s.unclear ? '（待確認）' : ''} | ${summarize(statuses)} |`)
    for (const p of [s.feature, ...s.tests]) if (p && !existsSync(join(srcRoot, p))) problems.push(`${page.title}：「${s.name}」列出的 frontend/src/${p} 不存在`)
    if (s.unclear) unclear.push(`${page.title} ${s.name}：${s.unclear}`)
  }
  out('')
  const overlays = sections.filter((sec) => sec.name.startsWith('Overlays'))
  if (overlays.length) {
    out(`覆蓋層（殼層 #${overlaySectionIssue}，不逐一列出）：${overlays.map((sec) => `${sec.name} ${nodeLink(sec.id)}`).join('、')}`, '')
  }
  const proposalSections = sections.filter((sec) => /提案/.test(sec.name))
  const otherSections = sections.filter((sec) => !overlays.includes(sec) && !proposalSections.includes(sec) && !frames.some((f) => f.section === sec))
  if (otherSections.length) out(`其他區塊（不是畫面）：${otherSections.map((sec) => `${sec.name} ${nodeLink(sec.id)}`).join('、')}`, '')

  out('### 各畫面的 frame', '')
  for (const s of page.screens) {
    const states = byScreen.get(s)
    out(`#### ${s.name}`, '')
    if (s.note) out(s.note, '')
    out('| 狀態 | 1440 | 768 | 375 | 狀態 |', '| --- | --- | --- | --- | --- |')
    for (const [state, row] of states) {
      const st = statusOf(s, state)
      // 導覽抽屜、側欄展開、平面圖面板本來就只存在於單一寬度
      const missing = /導覽抽屜|側欄展開|導覽選單開啟|平面圖面板/.test(state) ? [] : WIDTHS.filter((w) => !row[w])
      const label = STATUS[st] + (missing.length && st !== 'proposal' ? `；缺 ${missing.join('、')}` : '')
      out(`| ${state} | ${WIDTHS.map((w) => (row[w] ? nodeLink(row[w]) : '—')).join(' | ')} | ${label} |`)
      if (st === 'old') notRedrawn.push(`${page.title} ${state}（${issueLink(s.issue)}）`)
    }
    out('')
  }
}

out('## 尚未重畫的狀態（彙整）', '', '實作時只參考內容；載入中用 skeleton，失敗用 Alert，版面沿用同頁已重畫的畫面。', '')
for (const n of notRedrawn) out(`- ${n}`)
out('- [04] 成果 R02 活動詳細 · 空狀態（#685）：Figma 沒有畫')
out('', '## 待確認：路由或負責 Issue 不明確', '')
for (const u of unclear) out(`- ${u}`)
out('', '## 沒有 Figma 畫面的路由', '', '沿用 Vedute 元件與殼層（#670–#672）改版，不另畫。', '', '| 路由 | 檔案 | 用途 |', '| --- | --- | --- |')
for (const [route, file, what] of routesWithoutFigma) {
  out(`| ${route.startsWith('/') ? code(route) : route} | ${code(file.replace(/^features\//, ''))} | ${what} |`)
  if (!existsSync(join(srcRoot, file))) problems.push(`沒有 Figma 畫面的路由：frontend/src/${file} 不存在`)
}
out('')

const doc = lines.join('\n')
if (process.argv.includes('--check')) {
  const current = existsSync(outPath) ? readFileSync(outPath, 'utf8') : ''
  if (current !== doc) problems.push('docs/design/figma-map.md 不是最新的，請執行 node scripts/figma-map/build.mjs')
} else {
  writeFileSync(outPath, doc)
  console.log(`已寫入 ${outPath}`)
}
if (problems.length) {
  console.error(problems.map((p) => `✗ ${p}`).join('\n'))
  process.exit(1)
}
