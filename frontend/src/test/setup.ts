import '@testing-library/jest-dom/vitest'
import { cleanup, configure } from '@testing-library/react'
import { afterAll, afterEach, beforeAll, beforeEach, expect } from 'vitest'
import { server } from '@/test/server'
import { CONTRACT_ENFORCED, CONTRACT_EXEMPT_FILES, contractViolations } from '@/test/contract'

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

// 前後端契約：每個 MSW 假回應都用 docs/api/openapi.json 驗證；測試檔路徑在 CONTRACT_ENFORCED（且不在豁免清單）時有不符就失敗。
// 限制：只檢查 JSON 回應；非 JSON（純文字、204、串流）與 Playwright 用的瀏覽器 MSW（src/test/browser.ts）不檢查。
const pendingChecks: Promise<string[]>[] = []
// 測試結束、元件卸載之後才完成的回應（被中止的請求）不歸給任何測試，否則會算到下一個測試頭上
let accepting = false
beforeEach(() => {
  accepting = true
})

/** 檢查器自己丟的例外（契約或程式本身的問題）當成違反回報，不可悄悄通過。 */
const guarded = (run: () => Promise<string[]>) => run().catch((e: unknown) => [`契約檢查器內部錯誤：${String(e)}`])

server.events.on('response:mocked', ({ request, response }) => {
  const { pathname } = new URL(request.url)
  if (!accepting || !pathname.startsWith('/api/') || !(response.headers.get('content-type') ?? '').includes('json')) return
  const copy = response.clone()
  pendingChecks.push(
    guarded(async () => {
      const text = await copy.text()
      if (text === '') return []
      let body: unknown
      try {
        body = JSON.parse(text)
      } catch {
        return [`${request.method} ${pathname}：回應宣告為 JSON 但無法解析`]
      }
      return contractViolations(request.method, pathname, response.status, body)
    }),
  )
})

// 沒有被處理的請求一律報錯，避免測試悄悄打到真的網路
beforeAll(() => server.listen({ onUnhandledRequest: 'error' }))
afterEach(async () => {
  const violations = [...new Set((await Promise.all(pendingChecks.splice(0))).flat())]
  accepting = false
  const testPath = expect.getState().testPath
  cleanup()
  server.resetHandlers()
  localStorage.clear()
  sessionStorage.clear()
  // 取不到測試檔路徑就無法判斷要不要強制檢查；寧可大聲失敗，不能悄悄略過
  if (testPath === undefined) throw new Error('取不到 testPath，無法套用 MSW 契約檢查')
  const file = testPath.replaceAll('\\', '/')
  if (violations.length > 0 && CONTRACT_ENFORCED.some((dir) => file.includes(dir)) && !CONTRACT_EXEMPT_FILES.some((f) => file.endsWith(f))) {
    throw new Error(`MSW 假回應不符合 docs/api/openapi.json：\n${violations.join('\n')}`)
  }
})
afterAll(() => server.close())
