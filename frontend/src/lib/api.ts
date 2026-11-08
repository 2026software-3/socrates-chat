/**
 * 後端 API 客戶端。前端只經由同網域的 `/api` 存取資料（S-08.1）；認證靠 HttpOnly cookie，
 * 前端不持有任何 token。錯誤只有 `code` 與 `request_id`，沒有已翻譯文字（S-12.1）。
 */

export class ApiError extends Error {
  readonly status: number
  readonly code: string
  readonly requestId: string

  constructor(status: number, code: string, requestId: string) {
    super(code)
    this.name = 'ApiError'
    this.status = status
    this.code = code
    this.requestId = requestId
  }
}

import { markAiUsageStale } from '@/lib/ai-usage-events'

type Method = 'GET' | 'POST' | 'PATCH' | 'PUT' | 'DELETE'

/** 寫入類請求結束後通知用量顯示重新載入：任何一個都可能呼叫了 AI（教師翻譯、重新分組等），成功或失敗都一樣。 */
async function request<T>(method: Method, path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
  try {
    return await send<T>(method, path, body, signal)
  } finally {
    if (method !== 'GET') markAiUsageStale()
  }
}

async function send<T>(method: Method, path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
  let res: Response
  try {
    res = await fetch(path, {
      method,
      credentials: 'same-origin',
      headers: {
        Accept: 'application/json',
        ...(body === undefined ? {} : { 'Content-Type': 'application/json' }),
      },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal,
    })
  } catch (e) {
    if (isAbortError(e)) throw e
    // 網路中斷或伺服器沒回應
    throw new ApiError(0, 'network', '')
  }
  if (res.ok) {
    if (res.status === 204) return undefined as T
    const text = await res.text()
    return (text ? JSON.parse(text) : undefined) as T
  }
  throw await apiErrorFrom(res)
}

/** 由失敗的回應組出 `ApiError`：優先取 JSON 內容的 `error`，否則退回 `x-request-id` 標頭。 */
export async function apiErrorFrom(res: Response): Promise<ApiError> {
  let code = 'internal'
  let requestId = res.headers.get('x-request-id') ?? ''
  try {
    const data = await res.json()
    code = data?.error?.code ?? code
    requestId = data?.error?.request_id ?? requestId
  } catch {
    // 非 JSON 錯誤（例如反向代理的 502）
  }
  return new ApiError(res.status, code, requestId)
}

export const api = {
  get: <T>(path: string, signal?: AbortSignal) => request<T>('GET', path, undefined, signal),
  post: <T>(path: string, body?: unknown) => request<T>('POST', path, body),
  put: <T = void>(path: string, body?: unknown) => request<T>('PUT', path, body),
  patch: <T>(path: string, body?: unknown) => request<T>('PATCH', path, body),
  delete: <T = void>(path: string, body?: unknown) => request<T>('DELETE', path, body),
}

export function isApiError(e: unknown): e is ApiError {
  return e instanceof ApiError
}

/** 分頁游標已失效（重送同一個游標不會成功，要從第一頁重新開始）。 */
export function isInvalidCursor(e: unknown): boolean {
  return isApiError(e) && e.code === 'invalid_cursor'
}

/** fetch 被 `AbortSignal` 中止時丟出的錯誤。 */
export function isAbortError(e: unknown): boolean {
  return e instanceof DOMException && e.name === 'AbortError'
}
