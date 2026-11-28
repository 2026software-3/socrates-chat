import '@testing-library/jest-dom/vitest'
import { cleanup, configure } from '@testing-library/react'
import { afterAll, afterEach, beforeAll, beforeEach, expect } from 'vitest'
import { server } from '@/test/server'

// findBy／waitFor 預設只等 1 秒。每個測試的第一個 findBy 要等整個 App 啟動（登入狀態、外框、延遲載入的路由、資料請求），
// 整套測試並行、機器忙碌時（CI、開著別的程式）常常超過 1 秒而偶發失敗。通過的測試不會因此變慢，
// 只有真的壞掉的測試要多等一下才失敗。單一測試的逾時（vite.config.ts 的 testTimeout）必須比它長。
configure({ asyncUtilTimeout: 5000 })

// jsdom 沒有 ResizeObserver / scrollTo / scrollIntoView，assistant-ui 的 Viewport 與輸入框會用到
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
if (typeof globalThis.ResizeObserver === 'undefined') {
  globalThis.ResizeObserver = ResizeObserverStub as unknown as typeof ResizeObserver
}
if (typeof Element.prototype.scrollTo !== 'function') {
  Element.prototype.scrollTo = () => {}
}
if (typeof Element.prototype.scrollIntoView !== 'function') {
  Element.prototype.scrollIntoView = () => {}
}

// 前後端契約：每個 MSW 假回應與前端送出的 JSON 請求本文，都用 docs/api/openapi.json 驗證，有不符就讓該測試失敗。
// 限制：只檢查 JSON（非 JSON 的回應、語音音檔等請求本文不檢查），也不涵蓋 Playwright 用的瀏覽器 MSW（src/test/browser.ts）。
// 契約檢查器（19k 行的快照＋Ajv）等到第一個 /api JSON 請求才載入，不碰 API 的測試檔不用付這筆啟動成本。

/** 不檢查的測試檔：用虛構端點（`/api/x`、`/api/ok`…）測 HTTP 客戶端本身，不是真實 API。 */
const CONTRACT_EXEMPT_FILE = 'src/lib/api.test.ts'

type Contract = typeof import('@/test/contract')
const pendingChecks: Promise<string[]>[] = []
// 只在要檢查的測試內收集；測試結束、元件卸載之後才完成的回應（被中止的請求）不歸給任何測試，否則會算到下一個測試頭上
let accepting = false
beforeEach(() => {
  const testPath = expect.getState().testPath
  // 取不到測試檔路徑就無法判斷要不要檢查；寧可大聲失敗，不能悄悄略過
  if (testPath === undefined) throw new Error('取不到 testPath，無法套用 MSW 契約檢查')
  accepting = !testPath.replaceAll('\\', '/').endsWith(CONTRACT_EXEMPT_FILE)
})

/** 這個請求（或回應）要不要檢查：收集中、是 `/api/` 底下、且本文宣告為 JSON。 */
const shouldCheck = (pathname: string, headers: Headers) =>
  accepting && pathname.startsWith('/api/') && (headers.get('content-type') ?? '').includes('json')

/**
 * 在 handler 讀走本文之前就複製，之後才解析並驗證。檢查器自己丟的例外（契約或程式本身的問題）
 * 當成違反回報，不可悄悄通過。
 */
function queueCheck(source: Request | Response, label: string, validate: (contract: Contract, body: unknown) => string[]) {
  const copy = source.clone()
  pendingChecks.push(
    (async () => {
      const text = await copy.text()
      if (text === '') return []
      let body: unknown
      try {
        body = JSON.parse(text)
      } catch {
        return [`${label}：宣告為 JSON 但無法解析`]
      }
      return validate(await import('@/test/contract'), body)
    })().catch((e: unknown) => [`契約檢查器內部錯誤：${String(e)}`]),
  )
}

server.events.on('request:start', ({ request }) => {
  const { pathname } = new URL(request.url)
  if (!shouldCheck(pathname, request.headers)) return
  queueCheck(request, `${request.method} ${pathname} 的請求`, (c, body) => c.requestViolations(request.method, pathname, body))
})
server.events.on('response:mocked', ({ request, response }) => {
  const { pathname } = new URL(request.url)
  if (!shouldCheck(pathname, response.headers)) return
  queueCheck(response, `${request.method} ${pathname} 的回應`, (c, body) => c.contractViolations(request.method, pathname, response.status, body))
})

// 沒有被處理的請求一律報錯，避免測試悄悄打到真的網路
beforeAll(() => server.listen({ onUnhandledRequest: 'error' }))
afterEach(async () => {
  const violations = [...new Set((await Promise.all(pendingChecks.splice(0))).flat())]
  accepting = false
  cleanup()
  server.resetHandlers()
  localStorage.clear()
  sessionStorage.clear()
  if (violations.length > 0) throw new Error(`MSW 假回應不符合 docs/api/openapi.json：\n${violations.join('\n')}`)
})
afterAll(() => server.close())
