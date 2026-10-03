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

type Method = 'GET' | 'POST' | 'PATCH' | 'PUT' | 'DELETE'

async function request<T>(method: Method, path: string, body?: unknown, signal?: AbortSignal): Promise<T> {
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
    if (e instanceof DOMException && e.name === 'AbortError') throw e
    // 網路中斷或伺服器沒回應
    throw new ApiError(0, 'network', '')
  }
  if (res.ok) {
    if (res.status === 204) return undefined as T
    const text = await res.text()
    return (text ? JSON.parse(text) : undefined) as T
  }
  let code = 'internal'
  let requestId = res.headers.get('x-request-id') ?? ''
  try {
    const data = await res.json()
    code = data?.error?.code ?? code
    requestId = data?.error?.request_id ?? requestId
  } catch {
    // 非 JSON 錯誤（例如反向代理的 502）
  }
  throw new ApiError(res.status, code, requestId)
}

export const api = {
  get: <T>(path: string, signal?: AbortSignal) => request<T>('GET', path, undefined, signal),
  post: <T>(path: string, body?: unknown) => request<T>('POST', path, body),
  patch: <T>(path: string, body?: unknown) => request<T>('PATCH', path, body),
  delete: <T = void>(path: string) => request<T>('DELETE', path),
}

export function isApiError(e: unknown): e is ApiError {
  return e instanceof ApiError
}
