import { Ajv2020 } from 'ajv/dist/2020'
import type { ErrorObject, ValidateFunction } from 'ajv'
import openapi from '../../../docs/api/openapi.json'

/**
 * 前後端契約：用後端產生並進版控的 OpenAPI 快照（`docs/api/openapi.json`，
 * 由 `backend/tests/openapi_contract.rs` 的 snapshot 測試保證與程式一致）驗證 MSW 假後端的回應與請求。
 */

type Content = Record<string, unknown>
type Operation = {
  requestBody?: { content?: Content }
  parameters?: { name: string; in: string; required?: boolean }[]
  responses?: Record<string, { content?: Content }>
}
const paths = openapi.paths as unknown as Record<string, Record<string, Operation>>

const ajv = new Ajv2020({ strict: false, validateFormats: false, allErrors: true })
ajv.addSchema(openapi, 'openapi')

const pointer = (...segments: string[]) =>
  segments.map((s) => '/' + encodeURIComponent(s.replaceAll('~', '~0').replaceAll('/', '~1'))).join('')

/** `/api/books/:id` 與 `/api/books/{book_id}` 視為同一個路徑。 */
const normalize = (path: string) => path.replace(/:[A-Za-z_]+/g, '{}').replace(/\{[^}]+\}/g, '{}')

const specPathsByTemplate = new Map(Object.keys(paths).map((p) => [normalize(p), p]))
const specPathPatterns = Object.keys(paths)
  .filter((p) => p.includes('{'))
  .map((p) => ({ specPath: p, pattern: new RegExp('^' + p.split(/\{[^}]+\}/).map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('[^/]+') + '$') }))

/** 路徑可以是 handler 的樣板（`/api/books/:id`）或實際請求路徑（`/api/books/12`）。 */
export function findOperation(method: string, path: string) {
  const specPath =
    specPathsByTemplate.get(normalize(path)) ??
    specPathPatterns.find(({ pattern }) => pattern.test(path.replace(/:[A-Za-z_]+/g, 'x')))?.specPath
  const operation = specPath ? paths[specPath][method.toLowerCase()] : undefined
  return specPath && operation ? { specPath, operation } : undefined
}

/** 維護模式的閘門（backend/src/lib.rs `maintenance_exempt`）放行的路徑；其餘 `/api` 路徑在維護中一律回 503 `maintenance`。 */
const MAINTENANCE_EXEMPT = /^\/api\/(announcements|openapi\.json|auth\/(login|logout|google\/.+)|docs.*)$/

/** 同一個 schema 只編譯一次（每個回應與請求都會驗證，不快取會重複編譯）。 */
const validators = new Map<string, ValidateFunction>()
function validatorFor(ref: string) {
  let validate = validators.get(ref)
  if (!validate) {
    validate = ajv.compile({ $ref: ref })
    validators.set(ref, validate)
  }
  return validate
}

const formatErrors = (label: string, errors: ErrorObject[] | null | undefined) =>
  (errors ?? []).map((e) => `${label}：${e.instancePath || '/'} ${e.message}`)

/**
 * `base`（例如 `['paths', '/api/me', 'get', 'responses', '200']`）底下的 JSON 內容是否容得下 `body`。
 * 沒有 JSON 內容就不檢查；契約寫了 JSON 卻沒有 schema 時無法驗證，當成契約缺漏而不是悄悄通過。
 */
function jsonViolations(label: string, content: Content | undefined, base: string[], body: unknown): string[] {
  const json = content?.['application/json'] as { schema?: unknown } | undefined
  if (!json) return []
  if (!json.schema) return [`${label}：契約的 JSON 內容沒有 schema`]
  const validate = validatorFor(`openapi#${pointer(...base, 'content', 'application/json', 'schema')}`)
  return validate(body) ? [] : formatErrors(label, validate.errors)
}

/** OpenAPI 的狀態碼可以是確切的 `200`、範圍 `2XX` 或 `default`。 */
function responseFor(operation: Operation, status: number) {
  const responses = operation.responses ?? {}
  const key = [String(status), `${Math.floor(status / 100)}XX`, 'default'].find((k) => k in responses)
  return key ? { key, response: responses[key] } : undefined
}

/** 回傳違反契約的訊息；空陣列代表符合。未記載的端點或狀態碼也算違反。 */
export function contractViolations(method: string, path: string, status: number, body: unknown): string[] {
  const found = findOperation(method, path)
  if (!found) return [`契約沒有 ${method.toUpperCase()} ${path}`]
  const matched = responseFor(found.operation, status)
  // 維護閘門是跨端點的 middleware，不會寫在每個端點的契約裡；回應仍要是標準錯誤格式，且 code 必須是 maintenance
  if (!matched && status === 503 && !MAINTENANCE_EXEMPT.test(found.specPath)) {
    const code = (body as { error?: { code?: unknown } } | null)?.error?.code
    if (code !== 'maintenance') return [`${method.toUpperCase()} ${found.specPath} 的 503 沒有記載，且 code 不是 maintenance（是 ${String(code)}）`]
    const validate = validatorFor('openapi#/components/schemas/ErrorBody')
    return validate(body) ? [] : formatErrors('維護中的 503', validate.errors)
  }
  if (!matched) return [`契約沒有記載 ${method.toUpperCase()} ${found.specPath} 的 ${status} 回應`]
  const base = ['paths', found.specPath, method.toLowerCase(), 'responses', matched.key]
  return jsonViolations(`${method.toUpperCase()} ${found.specPath} ${matched.key}`, matched.response.content, base, body)
}

/**
 * 前端送給 MSW handler 的 JSON 請求本文是否符合契約的 requestBody。
 * 不是 JSON 的本文（語音音檔等）不檢查；端點不在契約裡的由回應檢查回報，這裡略過。
 */
export function requestViolations(method: string, path: string, body: unknown): string[] {
  const found = findOperation(method, path)
  if (!found) return []
  const label = `${method.toUpperCase()} ${found.specPath} 的請求`
  const requestBody = found.operation.requestBody
  if (!requestBody?.content) return [`${label}：契約沒有記載 requestBody，但送出了本文`]
  return jsonViolations(label, requestBody.content, ['paths', found.specPath, method.toLowerCase(), 'requestBody'], body)
}
