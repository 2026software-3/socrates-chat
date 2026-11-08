import '@testing-library/jest-dom/vitest'
import { cleanup, configure } from '@testing-library/react'
import { afterAll, afterEach, beforeAll } from 'vitest'
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

// 沒有被處理的請求一律報錯，避免測試悄悄打到真的網路
beforeAll(() => server.listen({ onUnhandledRequest: 'error' }))
afterEach(() => {
  cleanup()
  server.resetHandlers()
  localStorage.clear()
  sessionStorage.clear()
})
afterAll(() => server.close())
