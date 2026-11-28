import { http, HttpResponse } from 'msw'
import { makeDetail, makeMessage, makeRadar, makeSummary } from '@/features/chat/test-data'

/** Synthetic browser-only scenarios for #635. Never imported by the production build. */
export const stanceRadarHandlers = [
  http.get('/api/conversations/stance-:scenario', ({ params }) => HttpResponse.json(makeDetail({
    id: `stance-${String(params.scenario)}`, title: '合成立場討論', status: 'ended', stage: 3,
    ended_at: '2026-03-05T09:00:00Z', converge_ready: true,
  }, [makeMessage({ content: '我反對第一點，但不在乎第二點。' }), makeMessage({ id: 'm2', role: 'ai', content: '謝謝你的思考，討論到此結束。' })]))),
  http.get('/api/conversations/stance-:scenario/summary', () => HttpResponse.json(makeSummary())),
  http.get('/api/conversations/stance-:scenario/radar', ({ params }) => {
    const scores: Record<string, number> = params.scenario === 'unknown' ? {} : params.scenario === 'partial'
      ? { d1: -2, d2: 0, d3: 1, d5: 2 }
      : { d1: -2, d2: -1, d3: 0, d4: 1, d5: 2, d6: 0 }
    return HttpResponse.json(makeRadar({ scale: 'stance', rules_version: 'radar-stance-v1', scores }))
  }),
]