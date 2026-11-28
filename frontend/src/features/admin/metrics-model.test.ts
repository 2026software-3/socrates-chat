import { describe, expect, it } from 'vitest'
import { estimateHistogramQuantile, parsePrometheusText, summarizeMetrics } from '@/features/admin/metrics-model'

const SAMPLE_EXPOSITION = [
  '# HELP socrates_build_info Build version information.',
  '# TYPE socrates_build_info gauge',
  'socrates_build_info{version="6fc831d"} 1',
  '# HELP socrates_process_uptime_seconds Process uptime in seconds.',
  '# TYPE socrates_process_uptime_seconds gauge',
  'socrates_process_uptime_seconds 7260',
  '# HELP socrates_maintenance_mode Maintenance flag.',
  '# TYPE socrates_maintenance_mode gauge',
  'socrates_maintenance_mode 0',
  '# HELP socrates_last_cleanup_timestamp_seconds Last cleanup timestamp.',
  '# TYPE socrates_last_cleanup_timestamp_seconds gauge',
  'socrates_last_cleanup_timestamp_seconds 1763773200',
  '# HELP socrates_db_pool_connections Pool counts.',
  '# TYPE socrates_db_pool_connections gauge',
  'socrates_db_pool_connections{state="active"} 2',
  'socrates_db_pool_connections{state="idle"} 6',
  'socrates_db_pool_connections{state="max"} 10',
  'socrates_db_pool_connections{state="size"} 8',
  '# TYPE socrates_live_replies_active gauge',
  'socrates_live_replies_active 3',
  '# TYPE socrates_voice_sessions_active gauge',
  'socrates_voice_sessions_active 1',
  '# TYPE socrates_service_up gauge',
  'socrates_service_up{service="asr"} 1',
  'socrates_service_up{service="tts"} 0',
  '# TYPE socrates_http_requests_in_flight gauge',
  'socrates_http_requests_in_flight 4',
  '# TYPE socrates_http_requests_total counter',
  'socrates_http_requests_total{method="GET",route="/api/catalog",status_class="2xx"} 90',
  'socrates_http_requests_total{method="POST",route="/api/conversations/{id}/messages",status_class="2xx"} 18',
  'socrates_http_requests_total{method="POST",route="/api/conversations/{id}/messages",status_class="5xx"} 2',
  'socrates_http_requests_total{method="GET",route="/api/me",status_class="4xx"} 5',
  '# TYPE socrates_http_requests_aborted_total counter',
  'socrates_http_requests_aborted_total{method="POST",route="/api/conversations/{id}/messages"} 1',
  '# TYPE socrates_http_request_timeouts_total counter',
  'socrates_http_request_timeouts_total{route="/api/conversations/{id}/messages"} 1',
  '# TYPE socrates_http_request_errors_total counter',
  'socrates_http_request_errors_total{route="/api/me",error_code="unauthorized"} 5',
  '# TYPE socrates_http_request_duration_seconds histogram',
  'socrates_http_request_duration_seconds_bucket{method="GET",route="/api/catalog",le="0.05"} 45',
  'socrates_http_request_duration_seconds_bucket{method="GET",route="/api/catalog",le="0.1"} 85',
  'socrates_http_request_duration_seconds_bucket{method="GET",route="/api/catalog",le="0.25"} 90',
  'socrates_http_request_duration_seconds_bucket{method="GET",route="/api/catalog",le="+Inf"} 90',
  'socrates_http_request_duration_seconds_sum{method="GET",route="/api/catalog"} 4.5',
  'socrates_http_request_duration_seconds_count{method="GET",route="/api/catalog"} 90',
  '# TYPE socrates_ai_requests_total counter',
  'socrates_ai_requests_total{function="chat_reply",outcome="ok"} 19',
  'socrates_ai_requests_total{function="chat_reply",outcome="timeout"} 1',
  '# TYPE socrates_ai_retries_total counter',
  'socrates_ai_retries_total{function="chat_reply"} 2',
  '# TYPE socrates_ai_request_duration_seconds histogram',
  'socrates_ai_request_duration_seconds_bucket{function="chat_reply",le="1"} 10',
  'socrates_ai_request_duration_seconds_bucket{function="chat_reply",le="2.5"} 20',
  'socrates_ai_request_duration_seconds_bucket{function="chat_reply",le="+Inf"} 20',
  'socrates_ai_request_duration_seconds_sum{function="chat_reply"} 22',
  'socrates_ai_request_duration_seconds_count{function="chat_reply"} 20',
  '# TYPE socrates_background_jobs_in_flight gauge',
  'socrates_background_jobs_in_flight{job="summary"} 1',
  '# TYPE socrates_background_jobs_total counter',
  'socrates_background_jobs_total{job="summary",outcome="ok"} 7',
  'socrates_background_jobs_total{job="summary",outcome="failed"} 1',
  '# TYPE socrates_service_probes_total counter',
  'socrates_service_probes_total{service="asr",outcome="ok"} 12',
  'socrates_service_probes_total{service="tts",outcome="timeout"} 3',
  '# TYPE socrates_translations_total counter',
  'socrates_translations_total{provider="google",outcome="ok"} 6',
  '# TYPE socrates_book_source_lookups_total counter',
  'socrates_book_source_lookups_total{provider="open_library",outcome="ok"} 4',
  '# TYPE socrates_complaint_mails_total counter',
  'socrates_complaint_mails_total{outcome="sent"} 2',
  'socrates_complaint_mails_total{outcome="failed"} 1',
  '# TYPE socrates_auth_events_total counter',
  'socrates_auth_events_total{method="password_login",outcome="ok"} 15',
  'socrates_auth_events_total{method="password_login",outcome="invalid_credentials"} 3',
  '# TYPE socrates_diagnostic_writes_dropped_total counter',
  'socrates_diagnostic_writes_dropped_total{anonymous="false"} 2',
].join('\n')

describe('parsePrometheusText', () => {
  it('handles empty and comment-only input', () => {
    expect(parsePrometheusText('')).toEqual([])
    expect(parsePrometheusText('# just a comment\n\n')).toEqual([])
  })

  it('unescapes label values containing quotes, commas, braces, and backslashes', () => {
    const families = parsePrometheusText(
      '# TYPE demo counter\ndemo{route="/api/{id}",note="a,b=\\"c\\"\\\\d\\ne"} 4.5e1 1700000000\n',
    )
    expect(families).toHaveLength(1)
    expect(families[0]?.samples[0]).toEqual({
      name: 'demo',
      labels: { route: '/api/{id}', note: 'a,b="c"\\d\ne' },
      value: 45,
    })
  })

  it('parses +Inf, -Inf, and NaN values without throwing', () => {
    const families = parsePrometheusText('a 1\nb +Inf\nc -Inf\nd NaN\n')
    expect(families.map((f) => f.samples[0]?.value)).toEqual([1, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY, Number.NaN])
  })

  it('groups histogram _bucket/_sum/_count even without # TYPE headers and does not hijack unrelated *_count counters', () => {
    const families = parsePrometheusText(
      [
        'socrates_http_request_duration_seconds_bucket{method="GET",route="/a",le="0.1"} 4',
        'socrates_http_request_duration_seconds_bucket{method="GET",route="/a",le="+Inf"} 4',
        'socrates_http_request_duration_seconds_sum{method="GET",route="/a"} 0.2',
        'socrates_http_request_duration_seconds_count{method="GET",route="/a"} 4',
        '# TYPE foo gauge',
        'foo 1',
        '# TYPE foo_count counter',
        'foo_count 9',
      ].join('\n'),
    )
    const hist = families.find((f) => f.name === 'socrates_http_request_duration_seconds')
    expect(hist?.type).toBe('histogram')
    expect(hist?.samples).toHaveLength(4)
    expect(families.find((f) => f.name === 'foo')?.samples).toHaveLength(1)
    expect(families.find((f) => f.name === 'foo_count')?.samples).toHaveLength(1)
  })
})

describe('estimateHistogramQuantile', () => {
  it('returns null for empty buckets, zero total, +Inf-only buckets, or NaN quantile', () => {
    expect(estimateHistogramQuantile([], 0.95)).toBeNull()
    expect(estimateHistogramQuantile([{ le: 0.1, count: 0 }, { le: Number.POSITIVE_INFINITY, count: 0 }], 0.95)).toBeNull()
    expect(estimateHistogramQuantile([{ le: Number.POSITIVE_INFINITY, count: 5 }], 0.95)).toBeNull()
    expect(estimateHistogramQuantile([{ le: 0.1, count: 5 }, { le: Number.POSITIVE_INFINITY, count: 5 }], Number.NaN)).toBeNull()
  })

  it('interpolates linearly inside the matching bucket', () => {
    const p95 = estimateHistogramQuantile(
      [
        { le: 0.05, count: 45 },
        { le: 0.1, count: 85 },
        { le: 0.25, count: 90 },
        { le: Number.POSITIVE_INFINITY, count: 90 },
      ],
      0.95,
    )
    // rank = 85.5, inside (0.1, 0.25] with delta 5 -> 0.1 + 0.15 * (0.5 / 5) = 0.115
    expect(p95).toBeCloseTo(0.115, 6)
  })
})

describe('summarizeMetrics', () => {
  it('summarizes process, live gauges, HTTP routes, AI, jobs, and dependencies', () => {
    const summary = summarizeMetrics(SAMPLE_EXPOSITION)
    expect(summary.process).toEqual({
      version: '6fc831d',
      uptimeSeconds: 7260,
      maintenanceOn: false,
      lastCleanupTimestamp: 1763773200,
    })
    expect(summary.live.httpInFlight).toBe(4)
    expect(summary.live.liveRepliesActive).toBe(3)
    expect(summary.live.voiceSessionsActive).toBe(1)
    expect(summary.live.backgroundJobsInFlight).toBe(1)
    expect(summary.live.dbPool).toEqual({ active: 2, idle: 6, max: 10, size: 8 })
    // Unhealthy services sort first
    expect(summary.live.servicesUp).toEqual([
      { service: 'tts', up: false },
      { service: 'asr', up: true },
    ])

    expect(summary.http.totalRequests).toBe(115)
    expect(summary.http.statusClasses).toEqual({ '2xx': 108, '3xx': 0, '4xx': 5, '5xx': 2, other: 0 })
    expect(summary.http.abortedTotal).toBe(1)
    expect(summary.http.timeoutsTotal).toBe(1)
    expect(summary.http.errorsTotal).toBe(5)
    expect(summary.http.topErrors).toEqual([{ code: 'unauthorized', count: 5 }])
    // Route with 5xx sorts first
    expect(summary.http.routes[0]).toMatchObject({
      method: 'POST',
      route: '/api/conversations/{id}/messages',
      total: 20,
      errors5xx: 2,
      aborted: 1,
    })
    const catalogRoute = summary.http.routes.find((r) => r.route === '/api/catalog')
    expect(catalogRoute).toMatchObject({ avgMs: 50, p95Ms: 115 })

    expect(summary.ai).toMatchObject({ totalCalls: 20, okCalls: 19, invalidOutputs: 0, failedCalls: 1, retriesTotal: 2 })
    expect(summary.ai.functions[0]).toMatchObject({ fn: 'chat_reply', p95Ms: 2350 })

    expect(summary.jobs).toMatchObject({ inFlightTotal: 1, totalRuns: 8 })
    expect(summary.jobs.items[0]).toMatchObject({ job: 'summary', inFlight: 1, ok: 7, degraded: 0, skipped: 0, failed: 1, aborted: 0 })

    expect(summary.dependencies.complaintMails).toEqual({ ok: 2, skipped: 0, failures: 1, total: 3 })
    expect(summary.dependencies.diagnosticWritesDropped).toBe(2)
    expect(summary.dependencies.authEvents[0]).toEqual({ name: 'password_login', total: 18, ok: 15, failures: 3 })
  })

  it('separates invalid_output, skipped, abandoned, degraded, and 1xx/<overflow> outcomes from failures', () => {
    const exposition = [
      'socrates_http_requests_total{method="GET",route="/api/voice/ws",status_class="1xx"} 4',
      'socrates_http_requests_total{method="<overflow>",route="<overflow>",status_class="<overflow>"} 3',
      'socrates_ai_requests_total{function="summary",outcome="ok"} 10',
      'socrates_ai_requests_total{function="summary",outcome="invalid_output"} 2',
      'socrates_ai_requests_total{function="summary",outcome="timeout"} 1',
      'socrates_background_jobs_total{job="complaint_mail",outcome="skipped"} 5',
      'socrates_background_jobs_total{job="reply",outcome="abandoned"} 3',
      'socrates_background_jobs_total{job="service_monitor",outcome="degraded"} 2',
      'socrates_background_jobs_total{job="reply",outcome="panicked"} 1',
      'socrates_complaint_mails_total{outcome="skipped"} 5',
      'socrates_complaint_mails_total{outcome="sent"} 1',
    ].join('\n')

    const summary = summarizeMetrics(exposition)
    expect(summary.http.totalRequests).toBe(7)
    expect(summary.http.statusClasses.other).toBe(7)
    expect(summary.ai).toMatchObject({ totalCalls: 13, okCalls: 10, invalidOutputs: 2, failedCalls: 1 })
    expect(summary.jobs.items.find((j) => j.job === 'complaint_mail')).toMatchObject({ skipped: 5, failed: 0 })
    expect(summary.jobs.items.find((j) => j.job === 'reply')).toMatchObject({ aborted: 3, failed: 1 })
    expect(summary.jobs.items.find((j) => j.job === 'service_monitor')).toMatchObject({ degraded: 2, failed: 0 })
    expect(summary.dependencies.complaintMails).toEqual({ ok: 1, skipped: 5, failures: 0, total: 6 })
  })
})
