import { lazy, type ComponentType } from 'react'

/**
 * 把模組裡具名匯出的頁面包成 lazy 元件：該模組（與只有它用到的依賴）等到第一次進入路由才下載。
 * 載入中的畫面與載入失敗的處理由 app/routes.tsx 的 `Suspense` 與 `RouteErrorBoundary` 負責。
 */
export function lazyPage<M extends Record<K, ComponentType>, K extends keyof M>(load: () => Promise<M>, name: K) {
  return lazy(() => load().then((m) => ({ default: m[name] })))
}
