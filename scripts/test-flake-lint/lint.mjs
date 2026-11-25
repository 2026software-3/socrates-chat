#!/usr/bin/env node
// 後端整合測試的「偶發失敗」寫法檢查（只用 Node 內建模組）。
//
//   node scripts/test-flake-lint/lint.mjs                   檢查（CI 用）
//   node scripts/test-flake-lint/lint.mjs --update-baseline 把目前的違規寫進基準檔
//
// 規則見 README.md。違規要嘗試改寫；確定沒問題才在該行或上一行加
// `// flaky-ok: <理由>`。基準檔只放歷史違規：新增的違規一律失敗，
// 已經修掉的項目也要從基準檔移除（只增不減的反向，只減不增）。

import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const ROOT = join(HERE, '..', '..')
const TESTS_DIR = 'backend/tests'
const BASELINE = join(HERE, 'baseline.txt')

const SLEEP_MS_THRESHOLD = 100

export const RULES = {
  'thread-sleep': {
    why: '用真實時間的 thread::sleep 判斷結果，機器一卡就失敗。注入時鐘，或把時間拉長到不受排程影響，或改成輪詢實際狀態。',
  },
  'long-sleep': {
    why: `固定睡 ${SLEEP_MS_THRESHOLD}ms 以上再斷言，等於賭背景工作在這段時間內做完。改成輪詢實際結果（有上限、逾時要 panic），或等明確的事件。`,
  },
  elapsed: {
    why: '用真實耗時做斷言（elapsed），負載高時會超標。上限要留足餘裕，或改成驗證行為而不是時間。',
  },
  'pool-acquire-count': {
    why: '數連線池被取用的次數，背景任務（如 request_stats）也會取用，計數會被污染。重複量測取最小值，或改驗行為。',
  },
}

const FLAKY_OK = /flaky-ok:\s*(\S.*)$/

function isComment(line) {
  return /^\s*\/\//.test(line)
}

function allowed(lines, index) {
  const same = FLAKY_OK.exec(lines[index])
  if (same && same[1].trim().length > 0) return true
  const above = lines[index - 1]
  if (above !== undefined && isComment(above)) {
    const match = FLAKY_OK.exec(above)
    return Boolean(match && match[1].trim().length > 0)
  }
  return false
}

function sleepMillis(line) {
  if (!/\bsleep\(/.test(line)) return null
  const millis = /Duration::from_millis\(\s*(\d[\d_]*)\s*\)/.exec(line)
  if (millis) return Number(millis[1].replaceAll('_', ''))
  if (/Duration::from_secs(_f(32|64))?\(/.test(line)) return Infinity
  return null
}

/** 掃一份原始碼，回傳 [{rule, line, text}]。 */
export function scan(source) {
  const lines = source.split('\n')
  const found = []
  lines.forEach((line, index) => {
    if (isComment(line)) return
    const rules = []
    if (/\bthread::sleep\(/.test(line)) rules.push('thread-sleep')
    const millis = sleepMillis(line)
    if (!/\bthread::sleep\(/.test(line) && millis !== null && millis >= SLEEP_MS_THRESHOLD) {
      rules.push('long-sleep')
    }
    if (/\.elapsed\(\)/.test(line)) rules.push('elapsed')
    if (/\bbefore_acquire\(/.test(line)) rules.push('pool-acquire-count')
    if (rules.length === 0 || allowed(lines, index)) return
    for (const rule of rules) found.push({ rule, line: index + 1, text: line.trim() })
  })
  return found
}

function rustFiles(dir) {
  const files = []
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name)
    if (entry.isDirectory()) files.push(...rustFiles(path))
    else if (entry.name.endsWith('.rs')) files.push(path)
  }
  return files.sort()
}

const key = (file, rule, text) => `${file}\t${rule}\t${text}`

/** 掃整個 backend/tests，和基準檔比對。baseline 是基準檔的各行。 */
export function check(root, baseline) {
  const found = []
  const dir = join(root, TESTS_DIR)
  if (existsSync(dir)) {
    for (const path of rustFiles(dir)) {
      const file = relative(root, path).split('\\').join('/')
      for (const f of scan(readFileSync(path, 'utf8'))) found.push({ file, ...f })
    }
  }
  const remaining = new Map()
  for (const line of baseline) remaining.set(line, (remaining.get(line) ?? 0) + 1)
  const newFindings = []
  for (const f of found) {
    const k = key(f.file, f.rule, f.text)
    const left = remaining.get(k) ?? 0
    if (left > 0) remaining.set(k, left - 1)
    else newFindings.push(f)
  }
  const stale = []
  for (const [k, count] of remaining) for (let i = 0; i < count; i++) stale.push(k)
  return { found, newFindings, stale }
}

function readBaseline() {
  if (!existsSync(BASELINE)) return []
  return readFileSync(BASELINE, 'utf8')
    .split('\n')
    .filter((line) => line.length > 0 && !line.startsWith('#'))
}

function main() {
  const update = process.argv.includes('--update-baseline')
  const { found, newFindings, stale } = check(ROOT, update ? [] : readBaseline())
  if (update) {
    const body = found.map((f) => key(f.file, f.rule, f.text)).sort()
    const header =
      '# 後端測試偶發失敗寫法的歷史違規（file<TAB>rule<TAB>該行內容）。只能減少，不要新增。\n' +
      '# 修掉一項後執行 node scripts/test-flake-lint/lint.mjs --update-baseline 重新產生。\n'
    writeFileSync(BASELINE, header + body.join('\n') + (body.length ? '\n' : ''))
    console.log(`基準檔已更新：${body.length} 項`)
    return
  }
  let failed = false
  for (const f of newFindings) {
    failed = true
    console.error(`${f.file}:${f.line}: [${f.rule}] ${RULES[f.rule].why}\n    ${f.text}`)
  }
  if (newFindings.length > 0) {
    console.error(
      '\n改寫成不依賴時間的寫法；確定沒問題才在該行或上一行加 `// flaky-ok: <理由>`（理由必填）。\n',
    )
  }
  for (const k of stale) {
    failed = true
    const [file, rule, text] = k.split('\t')
    console.error(`${file}: 基準檔裡的 [${rule}] 已不存在，請執行 --update-baseline 移除：${text}`)
  }
  if (failed) process.exit(1)
  console.log(`測試偶發寫法檢查通過（基準檔 ${readBaseline().length} 項）`)
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main()
