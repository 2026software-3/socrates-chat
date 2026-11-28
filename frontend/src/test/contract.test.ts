import { describe, expect, it } from 'vitest'
import { contractViolations, findOperation, requestViolations } from '@/test/contract'
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
    // 維護閘門只會回 maintenance；別的 code 沒記載就是違反
    expect(contractViolations('GET', '/api/me', 503, { error: { code: 'database_unavailable', request_id: 'r' } })).not.toEqual([])
  })

  it('請求本文：缺必填欄位、多餘欄位、型別錯誤都算違反', () => {
    const put = (body: unknown) => requestViolations('PUT', '/api/admin/users/11111111-1111-4111-8111-111111111111/status', body)
    expect(put({ disabled: true, reason: '測試' })).toEqual([])
    expect(put({ disabled: 'yes' })).not.toEqual([])
    expect(put({})).not.toEqual([])
    expect(requestViolations('GET', '/api/points', { x: 1 })).not.toEqual([])
    // 契約裡沒有的端點由回應檢查回報，請求檢查略過
    expect(requestViolations('POST', '/api/no-such-endpoint', { x: 1 })).toEqual([])
  })

  it('契約沒有的端點或狀態碼算違反', () => {
    expect(contractViolations('GET', '/api/no-such-endpoint', 200, {})).not.toEqual([])
    expect(contractViolations('GET', '/api/points', 418, {})).not.toEqual([])
  })
})
