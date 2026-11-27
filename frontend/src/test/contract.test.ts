import { describe, expect, it } from 'vitest'
import { contractViolations, findOperation } from '@/test/contract'
import { defaultHandlers } from '@/test/handlers'

describe('前後端契約：MSW 預設假端點符合後端 OpenAPI', () => {
  for (const handler of defaultHandlers) {
    const { method, path } = handler.info as { method: string; path: string }
    it(`${method} ${path}`, async () => {
      // 依契約補上必填的查詢參數（這個端點的 handler 會原樣回傳 provider）
      const required = (findOperation(method, path)?.operation.parameters ?? []).filter((p) => p.in === 'query' && p.required)
      const query = required.map((p) => `${p.name}=openai`).join('&')
      const url = path.replace(/:[A-Za-z_]+/g, '1') + (query ? `?${query}` : '')
      const response = await fetch(url, { method })
      const body = await response.json()
      expect(contractViolations(method, path, response.status, body)).toEqual([])
    })
  }

  it('偵測得到欄位型別錯誤與缺少的必填欄位', () => {
    expect(contractViolations('GET', '/api/points', 200, { balance: 'many' })).not.toEqual([])
    expect(contractViolations('GET', '/api/points', 200, {})).not.toEqual([])
    expect(contractViolations('GET', '/api/points', 200, { balance: 0 })).toEqual([])
  })

  it('維護中的 503 是跨端點的標準錯誤；閘門放行的路徑不適用', () => {
    const body = { error: { code: 'maintenance', request_id: 'r' } }
    expect(contractViolations('GET', '/api/me', 503, body)).toEqual([])
    expect(contractViolations('GET', '/api/me', 503, {})).not.toEqual([])
    expect(contractViolations('GET', '/api/announcements', 503, body)).not.toEqual([])
  })

  it('契約沒有的端點或狀態碼算違反', () => {
    expect(contractViolations('GET', '/api/no-such-endpoint', 200, {})).not.toEqual([])
    expect(contractViolations('GET', '/api/points', 418, {})).not.toEqual([])
  })
})
