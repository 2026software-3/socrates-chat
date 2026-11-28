/**
 * Prometheus Text Exposition Format 0.0.4 解析與彙總（搭配後端 `GET /api/admin/metrics`，S-09.3）。
 * 純函式、零額外依賴；所有指標皆為聚合計數／量表／直方圖，不含任何個資或對話原文（S-02.2）。
 */

export type MetricType = 'counter' | 'gauge' | 'histogram' | 'summary' | 'untyped'

export type MetricSample = {
  name: string
  labels: Record<string, string>
  value: number
}

export type MetricFamily = {
  name: string
  type: MetricType
  help: string
  samples: MetricSample[]
}

export type RouteMetricRow = {
  method: string
  route: string
  total: number
  errors5xx: number
  aborted: number
  avgMs: number | null
  p95Ms: number | null
}

export type AiMetricRow = {
  fn: string
  total: number
  ok: number
  invalidOutputs: number
  failures: number
  retries: number
  p95Ms: number | null
}

export type JobMetricRow = {
  job: string
  inFlight: number
  ok: number
  degraded: number
  skipped: number
  failed: number
  aborted: number
  p95Ms: number | null
}

export type ErrorCodeRow = {
  code: string
  count: number
}

export type OutcomeGroupRow = {
  name: string
  total: number
  ok: number
  failures: number
}

export type MetricsSummary = {
  familyCount: number
  sampleCount: number
  process: {
    version: string | null
    uptimeSeconds: number | null
    maintenanceOn: boolean | null
    lastCleanupTimestamp: number | null
  }
  live: {
    httpInFlight: number
    liveRepliesActive: number
    voiceSessionsActive: number
    backgroundJobsInFlight: number
    dbPool: { active: number; idle: number; max: number; size: number } | null
    servicesUp: { service: string; up: boolean }[]
  }
  http: {
    totalRequests: number
    statusClasses: { '2xx': number; '3xx': number; '4xx': number; '5xx': number; other: number }
    abortedTotal: number
    timeoutsTotal: number
    errorsTotal: number
    topErrors: ErrorCodeRow[]
    routes: RouteMetricRow[]
  }
  ai: {
    totalCalls: number
    okCalls: number
    invalidOutputs: number
    failedCalls: number
    retriesTotal: number
    functions: AiMetricRow[]
  }
  jobs: {
    inFlightTotal: number
    totalRuns: number
    items: JobMetricRow[]
  }
  dependencies: {
    serviceProbes: OutcomeGroupRow[]
    translations: OutcomeGroupRow[]
    bookSourceLookups: OutcomeGroupRow[]
    complaintMails: { ok: number; skipped: number; failures: number; total: number }
    authEvents: OutcomeGroupRow[]
    diagnosticWritesDropped: number
  }
}

function parseValue(raw: string): number | null {
  if (raw === '+Inf' || raw === 'Inf') return Number.POSITIVE_INFINITY
  if (raw === '-Inf') return Number.NEGATIVE_INFINITY
  if (raw === 'NaN') return Number.NaN
  const n = Number(raw)
  return Number.isNaN(n) ? null : n
}

function unescapeLabel(raw: string): string {
  let out = ''
  for (let i = 0; i < raw.length; i += 1) {
    if (raw[i] === '\\' && i + 1 < raw.length) {
      const next = raw[i + 1]
      if (next === 'n') out += '\n'
      else if (next === '"') out += '"'
      else if (next === '\\') out += '\\'
      else out += next
      i += 1
    } else {
      out += raw[i]
    }
  }
  return out
}

function parseLabels(src: string): Record<string, string> | null {
  const labels: Record<string, string> = {}
  let i = 0
  const len = src.length
  while (i < len) {
    while (i < len && (src[i] === ' ' || src[i] === '\t' || src[i] === ',')) i += 1
    if (i >= len) break
    const keyStart = i
    while (i < len && src[i] !== '=' && src[i] !== ' ' && src[i] !== '\t') i += 1
    const key = src.slice(keyStart, i)
    if (!key) return null
    while (i < len && (src[i] === ' ' || src[i] === '\t')) i += 1
    if (src[i] !== '=') return null
    i += 1
    while (i < len && (src[i] === ' ' || src[i] === '\t')) i += 1
    if (src[i] !== '"') return null
    i += 1
    const valStart = i
    while (i < len && src[i] !== '"') {
      if (src[i] === '\\' && i + 1 < len) i += 2
      else i += 1
    }
    if (i >= len) return null
    labels[key] = unescapeLabel(src.slice(valStart, i))
    i += 1
  }
  return labels
}

/** 解析 Prometheus Text Exposition Format 0.0.4。無法辨識的行會安全忽略。 */
export function parsePrometheusText(input: string): MetricFamily[] {
  const families = new Map<string, MetricFamily>()
  const ensure = (name: string): MetricFamily => {
    let f = families.get(name)
    if (!f) {
      f = { name, type: 'untyped', help: '', samples: [] }
      families.set(name, f)
    }
    return f
  }

  for (const rawLine of input.split(/\r?\n/)) {
    const line = rawLine.trim()
    if (!line) continue
    if (line.startsWith('#')) {
      const helpMatch = /^#\s+HELP\s+([a-zA-Z_:][a-zA-Z0-9_:]*)\s+(.*)$/.exec(line)
      if (helpMatch) {
        const [, name, help] = helpMatch
        if (name) ensure(name).help = help ?? ''
        continue
      }
      const typeMatch = /^#\s+TYPE\s+([a-zA-Z_:][a-zA-Z0-9_:]*)\s+(counter|gauge|histogram|summary|untyped)\b/.exec(line)
      if (typeMatch) {
        const [, name, type] = typeMatch
        if (name && type) ensure(name).type = type as MetricType
      }
      continue
    }

    const nameMatch = /^([a-zA-Z_:][a-zA-Z0-9_:]*)/.exec(line)
    if (!nameMatch) continue
    const sampleName = nameMatch[1]
    if (!sampleName) continue
    let cursor = sampleName.length
    let labels: Record<string, string> = {}

    if (line[cursor] === '{') {
      cursor += 1
      let inQuote = false
      const start = cursor
      while (cursor < line.length) {
        const ch = line[cursor]
        if (inQuote) {
          if (ch === '\\') cursor += 2
          else {
            if (ch === '"') inQuote = false
            cursor += 1
          }
        } else if (ch === '"') {
          inQuote = true
          cursor += 1
        } else if (ch === '}') {
          break
        } else {
          cursor += 1
        }
      }
      if (cursor >= line.length || line[cursor] !== '}') continue
      const parsed = parseLabels(line.slice(start, cursor))
      if (!parsed) continue
      labels = parsed
      cursor += 1
    }

    const rest = line.slice(cursor).trim()
    if (!rest) continue
    const valueToken = rest.split(/\s+/)[0]
    if (!valueToken) continue
    const value = parseValue(valueToken)
    if (value === null) continue

    // 直方圖／摘要的 _bucket / _sum / _count 歸在同一個 family 下：
    // - `_bucket` 帶 `le` 標籤時，即使省略了 `# TYPE` 標頭也歸到 base family（並標記為 histogram）
    // - `_sum` / `_count` 只在 base family 確實為 histogram 或 summary 時歸併，避免誤併無關的 `*_count` counter
    let familyName = sampleName
    if (sampleName.endsWith('_bucket') && 'le' in labels) {
      familyName = sampleName.slice(0, -'_bucket'.length)
      const fam = ensure(familyName)
      if (fam.type === 'untyped') fam.type = 'histogram'
    } else if (sampleName.endsWith('_sum') || sampleName.endsWith('_count')) {
      const base = sampleName.replace(/_(?:sum|count)$/, '')
      const baseFamily = families.get(base)
      if (baseFamily && (baseFamily.type === 'histogram' || baseFamily.type === 'summary')) {
        familyName = base
      }
    }
    ensure(familyName).samples.push({ name: sampleName, labels, value })
  }

  return Array.from(families.values())
}

/** 依 Prometheus 直方圖分桶線性內插估算分位數（單位與 bucket `le` 一致，無資料回傳 `null`）。 */
export function estimateHistogramQuantile(
  buckets: { le: number; count: number }[],
  quantile: number,
): number | null {
  if (buckets.length === 0 || !Number.isFinite(quantile) || quantile < 0 || quantile > 1) return null
  const sorted = [...buckets].sort((a, b) => a.le - b.le)
  const total = sorted[sorted.length - 1]?.count ?? 0
  if (total <= 0) return null
  const rank = quantile * total
  let prevBound = 0
  let prevCount = 0
  let hasFinite = false
  for (const bucket of sorted) {
    if (bucket.count >= rank) {
      if (!Number.isFinite(bucket.le)) return hasFinite ? prevBound : null
      const delta = bucket.count - prevCount
      if (delta <= 0) return bucket.le
      const ratio = (rank - prevCount) / delta
      return prevBound + (bucket.le - prevBound) * ratio
    }
    if (Number.isFinite(bucket.le)) {
      prevBound = bucket.le
      hasFinite = true
    }
    prevCount = bucket.count
  }
  return hasFinite ? prevBound : null
}

type HistogramGroup = {
  buckets: { le: number; count: number }[]
  sum: number
  count: number
}

function collectHistograms(
  samples: MetricSample[],
  baseName: string,
  keyFn: (labels: Record<string, string>) => string,
): Map<string, HistogramGroup> {
  const map = new Map<string, HistogramGroup>()
  const ensure = (key: string): HistogramGroup => {
    let g = map.get(key)
    if (!g) {
      g = { buckets: [], sum: 0, count: 0 }
      map.set(key, g)
    }
    return g
  }
  for (const s of samples) {
    const key = keyFn(s.labels)
    if (s.name === `${baseName}_bucket`) {
      const leRaw = s.labels.le
      if (leRaw === undefined) continue
      const le = parseValue(leRaw)
      if (le === null || Number.isNaN(le)) continue
      ensure(key).buckets.push({ le, count: s.value })
    } else if (s.name === `${baseName}_sum`) {
      ensure(key).sum = s.value
    } else if (s.name === `${baseName}_count`) {
      ensure(key).count = s.value
    }
  }
  return map
}

function finiteOrZero(n: number): number {
  return Number.isFinite(n) ? n : 0
}

function summarizeOutcomes(samples: MetricSample[], labelKey: string): OutcomeGroupRow[] {
  const map = new Map<string, OutcomeGroupRow>()
  for (const s of samples) {
    const name = s.labels[labelKey] ?? 'unknown'
    const outcome = s.labels.outcome ?? ''
    const row = map.get(name) ?? { name, total: 0, ok: 0, failures: 0 }
    const v = finiteOrZero(s.value)
    row.total += v
    if (outcome === 'ok' || outcome === 'healthy' || outcome === 'sent') row.ok += v
    else row.failures += v
    map.set(name, row)
  }
  return Array.from(map.values()).sort((a, b) => b.total - a.total || a.name.localeCompare(b.name))
}

/** 將 Prometheus 原始文字整理成後台「系統指標」頁面需要的結構化摘要。 */
export function summarizeMetrics(input: string): MetricsSummary {
  const families = parsePrometheusText(input)
  const byName = new Map(families.map((f) => [f.name, f]))
  const samplesOf = (name: string) => byName.get(name)?.samples ?? []
  const scalarOf = (name: string): number | null => {
    const first = samplesOf(name)[0]
    return first && Number.isFinite(first.value) ? first.value : null
  }

  const buildSample = samplesOf('socrates_build_info')[0]
  const maintenanceRaw = scalarOf('socrates_maintenance_mode')
  const lastCleanupRaw = scalarOf('socrates_last_cleanup_timestamp_seconds')

  // DB pool
  const poolSamples = samplesOf('socrates_db_pool_connections')
  let dbPool: MetricsSummary['live']['dbPool'] = null
  if (poolSamples.length > 0) {
    const byState = Object.fromEntries(poolSamples.map((s) => [s.labels.state ?? '', finiteOrZero(s.value)]))
    dbPool = {
      active: byState.active ?? 0,
      idle: byState.idle ?? 0,
      max: byState.max ?? 0,
      size: byState.size ?? 0,
    }
  }

  // Services up
  const servicesUp = samplesOf('socrates_service_up')
    .map((s) => ({ service: s.labels.service ?? 'unknown', up: s.value >= 1 }))
    .sort((a, b) => Number(a.up) - Number(b.up) || a.service.localeCompare(b.service))

  // HTTP totals & per-route breakdown
  const statusClasses = { '2xx': 0, '3xx': 0, '4xx': 0, '5xx': 0, other: 0 }
  const routeMap = new Map<string, RouteMetricRow>()
  const ensureRoute = (method: string, route: string): RouteMetricRow => {
    const key = `${method}\0${route}`
    let row = routeMap.get(key)
    if (!row) {
      row = { method, route, total: 0, errors5xx: 0, aborted: 0, avgMs: null, p95Ms: null }
      routeMap.set(key, row)
    }
    return row
  }

  let totalRequests = 0
  for (const s of samplesOf('socrates_http_requests_total')) {
    const v = finiteOrZero(s.value)
    totalRequests += v
    const cls = s.labels.status_class
    if (cls === '2xx' || cls === '3xx' || cls === '4xx' || cls === '5xx') statusClasses[cls] += v
    else statusClasses.other += v
    const row = ensureRoute(s.labels.method ?? '', s.labels.route ?? '')
    row.total += v
    if (cls === '5xx') row.errors5xx += v
  }

  let abortedTotal = 0
  for (const s of samplesOf('socrates_http_requests_aborted_total')) {
    const v = finiteOrZero(s.value)
    abortedTotal += v
    ensureRoute(s.labels.method ?? '', s.labels.route ?? '').aborted += v
  }

  const timeoutsTotal = samplesOf('socrates_http_request_timeouts_total').reduce((acc, s) => acc + finiteOrZero(s.value), 0)
  const errorCodeMap = new Map<string, number>()
  let errorsTotal = 0
  for (const s of samplesOf('socrates_http_request_errors_total')) {
    const v = finiteOrZero(s.value)
    errorsTotal += v
    const code = s.labels.error_code ?? 'unknown'
    errorCodeMap.set(code, (errorCodeMap.get(code) ?? 0) + v)
  }
  const topErrors: ErrorCodeRow[] = Array.from(errorCodeMap.entries())
    .filter(([, count]) => count > 0)
    .map(([code, count]) => ({ code, count }))
    .sort((a, b) => b.count - a.count || a.code.localeCompare(b.code))

  const httpHists = collectHistograms(
    samplesOf('socrates_http_request_duration_seconds'),
    'socrates_http_request_duration_seconds',
    (l) => `${l.method ?? ''}\0${l.route ?? ''}`,
  )
  for (const [key, hist] of httpHists) {
    const [method = '', route = ''] = key.split('\0')
    const row = ensureRoute(method, route)
    if (hist.count > 0) {
      row.avgMs = Math.round((hist.sum / hist.count) * 1000)
      const p95Sec = estimateHistogramQuantile(hist.buckets, 0.95)
      row.p95Ms = p95Sec === null ? null : Math.round(p95Sec * 1000)
    }
  }
  const routes = Array.from(routeMap.values()).sort(
    (a, b) => b.errors5xx - a.errors5xx || b.total - a.total || a.route.localeCompare(b.route),
  )

  // AI metrics (S-09.3: invalid_output 與供應商失敗分開計算)
  const aiMap = new Map<string, AiMetricRow>()
  const ensureAi = (fn: string): AiMetricRow => {
    let row = aiMap.get(fn)
    if (!row) {
      row = { fn, total: 0, ok: 0, invalidOutputs: 0, failures: 0, retries: 0, p95Ms: null }
      aiMap.set(fn, row)
    }
    return row
  }
  let aiTotal = 0
  let aiOk = 0
  let aiInvalidOutputs = 0
  let aiFailures = 0
  for (const s of samplesOf('socrates_ai_requests_total')) {
    const v = finiteOrZero(s.value)
    const row = ensureAi(s.labels.function ?? 'unknown')
    row.total += v
    aiTotal += v
    if (s.labels.outcome === 'ok') {
      row.ok += v
      aiOk += v
    } else if (s.labels.outcome === 'invalid_output') {
      row.invalidOutputs += v
      aiInvalidOutputs += v
    } else {
      row.failures += v
      aiFailures += v
    }
  }
  let aiRetries = 0
  for (const s of samplesOf('socrates_ai_retries_total')) {
    const v = finiteOrZero(s.value)
    aiRetries += v
    ensureAi(s.labels.function ?? 'unknown').retries += v
  }
  const aiHists = collectHistograms(
    samplesOf('socrates_ai_request_duration_seconds'),
    'socrates_ai_request_duration_seconds',
    (l) => l.function ?? 'unknown',
  )
  for (const [fn, hist] of aiHists) {
    const p95Sec = estimateHistogramQuantile(hist.buckets, 0.95)
    if (p95Sec !== null) ensureAi(fn).p95Ms = Math.round(p95Sec * 1000)
  }
  const aiFunctions = Array.from(aiMap.values()).sort(
    (a, b) => b.failures - a.failures || b.invalidOutputs - a.invalidOutputs || b.total - a.total,
  )

  // Background jobs
  const jobMap = new Map<string, JobMetricRow>()
  const ensureJob = (job: string): JobMetricRow => {
    let row = jobMap.get(job)
    if (!row) {
      row = { job, inFlight: 0, ok: 0, degraded: 0, skipped: 0, failed: 0, aborted: 0, p95Ms: null }
      jobMap.set(job, row)
    }
    return row
  }
  let jobsInFlightTotal = 0
  for (const s of samplesOf('socrates_background_jobs_in_flight')) {
    const v = finiteOrZero(s.value)
    jobsInFlightTotal += v
    ensureJob(s.labels.job ?? 'unknown').inFlight += v
  }
  let jobsTotalRuns = 0
  for (const s of samplesOf('socrates_background_jobs_total')) {
    const v = finiteOrZero(s.value)
    jobsTotalRuns += v
    const row = ensureJob(s.labels.job ?? 'unknown')
    const outcome = s.labels.outcome
    if (outcome === 'ok') row.ok += v
    else if (outcome === 'degraded') row.degraded += v
    else if (outcome === 'skipped') row.skipped += v
    else if (outcome === 'aborted' || outcome === 'abandoned') row.aborted += v
    else row.failed += v
  }
  const jobHists = collectHistograms(
    samplesOf('socrates_background_job_duration_seconds'),
    'socrates_background_job_duration_seconds',
    (l) => l.job ?? 'unknown',
  )
  for (const [job, hist] of jobHists) {
    const p95Sec = estimateHistogramQuantile(hist.buckets, 0.95)
    if (p95Sec !== null) ensureJob(job).p95Ms = Math.round(p95Sec * 1000)
  }
  const jobItems = Array.from(jobMap.values()).sort(
    (a, b) => b.inFlight - a.inFlight || b.failed - a.failed || b.degraded - a.degraded || a.job.localeCompare(b.job),
  )

  // Dependencies
  const complaintMails = { ok: 0, skipped: 0, failures: 0, total: 0 }
  for (const s of samplesOf('socrates_complaint_mails_total')) {
    const v = finiteOrZero(s.value)
    complaintMails.total += v
    if (s.labels.outcome === 'sent' || s.labels.outcome === 'ok') complaintMails.ok += v
    else if (s.labels.outcome === 'skipped') complaintMails.skipped += v
    else complaintMails.failures += v
  }

  const diagnosticWritesDropped = samplesOf('socrates_diagnostic_writes_dropped_total').reduce(
    (acc, s) => acc + finiteOrZero(s.value),
    0,
  )

  return {
    familyCount: families.length,
    sampleCount: families.reduce((acc, f) => acc + f.samples.length, 0),
    process: {
      version: buildSample?.labels.version ?? null,
      uptimeSeconds: scalarOf('socrates_process_uptime_seconds'),
      maintenanceOn: maintenanceRaw === null ? null : maintenanceRaw >= 1,
      lastCleanupTimestamp: lastCleanupRaw && lastCleanupRaw > 0 ? lastCleanupRaw : null,
    },
    live: {
      httpInFlight: scalarOf('socrates_http_requests_in_flight') ?? 0,
      liveRepliesActive: scalarOf('socrates_live_replies_active') ?? 0,
      voiceSessionsActive: scalarOf('socrates_voice_sessions_active') ?? 0,
      backgroundJobsInFlight: jobsInFlightTotal,
      dbPool,
      servicesUp,
    },
    http: {
      totalRequests,
      statusClasses,
      abortedTotal,
      timeoutsTotal,
      errorsTotal,
      topErrors,
      routes,
    },
    ai: {
      totalCalls: aiTotal,
      okCalls: aiOk,
      invalidOutputs: aiInvalidOutputs,
      failedCalls: aiFailures,
      retriesTotal: aiRetries,
      functions: aiFunctions,
    },
    jobs: {
      inFlightTotal: jobsInFlightTotal,
      totalRuns: jobsTotalRuns,
      items: jobItems,
    },
    dependencies: {
      serviceProbes: summarizeOutcomes(samplesOf('socrates_service_probes_total'), 'service'),
      translations: summarizeOutcomes(samplesOf('socrates_translations_total'), 'provider'),
      bookSourceLookups: summarizeOutcomes(samplesOf('socrates_book_source_lookups_total'), 'provider'),
      complaintMails,
      authEvents: summarizeOutcomes(samplesOf('socrates_auth_events_total'), 'method'),
      diagnosticWritesDropped,
    },
  }
}
