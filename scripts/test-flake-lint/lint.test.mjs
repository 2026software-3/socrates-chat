import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'

import { check, scan } from './lint.mjs'

const rules = (source) => scan(source).map((f) => f.rule)

test('thread::sleep in a test is flagged', () => {
  assert.deepEqual(rules('std::thread::sleep(Duration::from_millis(5));'), ['thread-sleep'])
  assert.deepEqual(rules('thread::sleep(d);'), ['thread-sleep'])
})

test('sleeps of 100ms or more are flagged, polling-sized sleeps are not', () => {
  assert.deepEqual(rules('tokio::time::sleep(Duration::from_millis(300)).await;'), ['long-sleep'])
  assert.deepEqual(rules('sleep(std::time::Duration::from_secs(1)).await;'), ['long-sleep'])
  assert.deepEqual(rules('tokio::time::sleep(Duration::from_millis(100)).await;'), ['long-sleep'])
  assert.deepEqual(rules('tokio::time::sleep(Duration::from_millis(99)).await;'), [])
  assert.deepEqual(rules('tokio::time::sleep(Duration::from_millis(10)).await;'), [])
})

test('sleeps whose length is a variable are not flagged', () => {
  assert.deepEqual(rules('tokio::time::sleep(delay).await;'), [])
})

test('elapsed() and before_acquire are flagged', () => {
  assert.deepEqual(rules('assert!(started.elapsed() < Duration::from_secs(8));'), ['elapsed'])
  assert.deepEqual(rules('.before_acquire(move |_, _| {'), ['pool-acquire-count'])
})

test('comments are ignored', () => {
  assert.deepEqual(rules('// tokio::time::sleep(Duration::from_millis(300)).await;'), [])
  assert.deepEqual(rules('    /// 數 before_acquire 的次數'), [])
})

test('flaky-ok with a reason on the same line or the line above allows it', () => {
  assert.deepEqual(
    rules('tokio::time::sleep(Duration::from_millis(300)).await; // flaky-ok: 負向檢查，變慢只會讓測試變弱'),
    [],
  )
  assert.deepEqual(
    rules(
      ['// flaky-ok: 負向檢查，變慢只會讓測試變弱', 'tokio::time::sleep(Duration::from_millis(300)).await;'].join('\n'),
    ),
    [],
  )
})

test('flaky-ok without a reason does not count', () => {
  assert.deepEqual(rules('std::thread::sleep(d); // flaky-ok'), ['thread-sleep'])
  assert.deepEqual(rules('std::thread::sleep(d); // flaky-ok:   '), ['thread-sleep'])
})

test('flaky-ok only covers the next line, not the ones after', () => {
  const source = ['// flaky-ok: 有理由', 'let a = 1;', 'std::thread::sleep(d);'].join('\n')
  assert.deepEqual(rules(source), ['thread-sleep'])
})

test('findings carry the line number and the trimmed source line', () => {
  const [finding] = scan('fn a() {}\n    std::thread::sleep(d);\n')
  assert.equal(finding.line, 2)
  assert.equal(finding.text, 'std::thread::sleep(d);')
})

// ---- 基準檔（只增不減）----

function repo(files, baseline) {
  const dir = mkdtempSync(join(tmpdir(), 'flake-lint-'))
  mkdirSync(join(dir, 'backend/tests'), { recursive: true })
  for (const [name, content] of Object.entries(files)) {
    writeFileSync(join(dir, 'backend/tests', name), content)
  }
  return { dir, baseline, cleanup: () => rmSync(dir, { recursive: true, force: true }) }
}

const LONG = 'tokio::time::sleep(Duration::from_millis(300)).await;'
const entry = (file, rule, text) => `${file}\t${rule}\t${text}`

test('a violation that is not in the baseline fails', () => {
  const r = repo({ 'a.rs': LONG }, [])
  try {
    const result = check(r.dir, r.baseline)
    assert.equal(result.newFindings.length, 1)
    assert.equal(result.newFindings[0].file, 'backend/tests/a.rs')
    assert.deepEqual(result.stale, [])
  } finally {
    r.cleanup()
  }
})

test('a violation listed in the baseline passes', () => {
  const r = repo({ 'a.rs': LONG }, [entry('backend/tests/a.rs', 'long-sleep', LONG)])
  try {
    const result = check(r.dir, r.baseline)
    assert.deepEqual(result.newFindings, [])
    assert.deepEqual(result.stale, [])
  } finally {
    r.cleanup()
  }
})

test('baseline entries survive line shifts', () => {
  const r = repo({ 'a.rs': `fn x() {}\n\n${LONG}` }, [entry('backend/tests/a.rs', 'long-sleep', LONG)])
  try {
    assert.deepEqual(check(r.dir, r.baseline).newFindings, [])
  } finally {
    r.cleanup()
  }
})

test('a second copy of a baselined line is a new violation', () => {
  const r = repo({ 'a.rs': `${LONG}\n${LONG}` }, [entry('backend/tests/a.rs', 'long-sleep', LONG)])
  try {
    assert.equal(check(r.dir, r.baseline).newFindings.length, 1)
  } finally {
    r.cleanup()
  }
})

test('a baseline entry whose violation is gone is reported as stale', () => {
  const r = repo({ 'a.rs': 'fn ok() {}' }, [entry('backend/tests/a.rs', 'long-sleep', LONG)])
  try {
    const result = check(r.dir, r.baseline)
    assert.deepEqual(result.newFindings, [])
    assert.equal(result.stale.length, 1)
  } finally {
    r.cleanup()
  }
})

test('nested directories under backend/tests are scanned', () => {
  const r = repo({}, [])
  try {
    mkdirSync(join(r.dir, 'backend/tests/common'))
    writeFileSync(join(r.dir, 'backend/tests/common/mod.rs'), LONG)
    assert.equal(check(r.dir, r.baseline).newFindings[0].file, 'backend/tests/common/mod.rs')
  } finally {
    r.cleanup()
  }
})
