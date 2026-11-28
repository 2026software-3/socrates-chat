import { Ajv2020 } from 'ajv/dist/2020'
import type { ValidateFunction } from 'ajv'
import openapi from '../../../docs/api/openapi.json'

/**
 * 前後端契約：用後端產生並進版控的 OpenAPI 快照（`docs/api/openapi.json`，
 * 由 `backend/tests/openapi_contract.rs` 的 snapshot 測試保證與程式一致）驗證 MSW 假後端的回應。
 */

type Operation = {
  parameters?: { name: string; in: string; required?: boolean }[]
  responses?: Record<string, { content?: Record<string, unknown> }>
}
const paths = openapi.paths as unknown as Record<string, Record<string, Operation>>

const ajv = new Ajv2020({ strict: false, validateFormats: false, allErrors: true })
ajv.addSchema(openapi, 'openapi')

const pointer = (...segments: string[]) =>
  segments.map((s) => '/' + encodeURIComponent(s.replaceAll('~', '~0').replaceAll('/', '~1'))).join('')

/** `/api/books/:id` 與 `/api/books/{book_id}` 視為同一個路徑。 */
const normalize = (path: string) => path.replace(/:[A-Za-z_]+/g, '{}').replace(/\{[^}]+\}/g, '{}')

const specPathPatterns = Object.keys(paths)
  .filter((p) => p.includes('{'))
  .map((p) => ({ specPath: p, pattern: new RegExp('^' + p.split(/\{[^}]+\}/).map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('[^/]+') + '$') }))

/** 路徑可以是 handler 的樣板（`/api/books/:id`）或實際請求路徑（`/api/books/12`）。 */
export function findOperation(method: string, path: string) {
  const wanted = normalize(path)
  const specPath =
    Object.keys(paths).find((p) => normalize(p) === wanted) ??
    specPathPatterns.find(({ pattern }) => pattern.test(path.replace(/:[A-Za-z_]+/g, 'x')))?.specPath
  const operation = specPath ? paths[specPath][method.toLowerCase()] : undefined
  return specPath && operation ? { specPath, operation } : undefined
}

/** 維護模式的閘門（backend/src/lib.rs `maintenance_gate`）放行的路徑；其餘 `/api` 路徑在維護中一律回 503 `maintenance`。 */
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

const errorBody = validatorFor('openapi#/components/schemas/ErrorBody')

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
    return errorBody(body) ? [] : [`維護中的 503：${errorBody.errors?.map((e) => `${e.instancePath || '/'} ${e.message}`).join('、')}`]
  }
  if (!matched) return [`契約沒有記載 ${method.toUpperCase()} ${found.specPath} 的 ${status} 回應`]
  const json = matched.response.content?.['application/json'] as { schema?: unknown } | undefined
  if (!json) return []
  // 契約寫了 JSON 回應卻沒有 schema：無法驗證，當成契約缺漏而不是悄悄通過
  if (!json.schema) return [`${method.toUpperCase()} ${found.specPath} ${matched.key}：契約的 JSON 回應沒有 schema`]
  const validate = validatorFor(
    `openapi#${pointer('paths', found.specPath, method.toLowerCase(), 'responses', matched.key, 'content', 'application/json', 'schema')}`,
  )
  return validate(body)
    ? []
    : (validate.errors ?? []).map((e) => `${method.toUpperCase()} ${found.specPath} ${matched.key}：${e.instancePath || '/'} ${e.message}`)
}

/**
 * 所有測試的 MSW 假回應都要符合契約（以測試檔路徑比對）；新目錄自動納入。
 * 確實不該檢查的測試檔放進 `CONTRACT_EXEMPT_FILES`，並寫明理由。
 */
export const CONTRACT_ENFORCED = ['src/']

/** 不檢查的測試檔：用虛構端點（`/api/x`、`/api/ok`…）測 HTTP 客戶端本身，不是真實 API。 */
export const CONTRACT_EXEMPT_FILES = ['src/lib/api.test.ts']
