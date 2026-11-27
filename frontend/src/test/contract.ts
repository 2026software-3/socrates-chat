import { Ajv2020 } from 'ajv/dist/2020'
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
  .map((p) => ({ specPath: p, pattern: new RegExp('^' + p.replace(/\{[^}]+\}/g, '[^/]+') + '$') }))

/** 路徑可以是 handler 的樣板（`/api/books/:id`）或實際請求路徑（`/api/books/12`）。 */
export function findOperation(method: string, path: string) {
  const wanted = normalize(path)
  const specPath =
    Object.keys(paths).find((p) => normalize(p) === wanted) ??
    specPathPatterns.find(({ pattern }) => pattern.test(path.replace(/:[A-Za-z_]+/g, 'x')))?.specPath
  const operation = specPath ? paths[specPath][method.toLowerCase()] : undefined
  return specPath && operation ? { specPath, operation } : undefined
}

/** 回傳違反契約的訊息；空陣列代表符合。未記載的端點或狀態碼也算違反。 */
export function contractViolations(method: string, path: string, status: number, body: unknown): string[] {
  const found = findOperation(method, path)
  if (!found) return [`契約沒有 ${method.toUpperCase()} ${path}`]
  const response = found.operation.responses?.[String(status)]
  if (!response) return [`契約沒有記載 ${method.toUpperCase()} ${found.specPath} 的 ${status} 回應`]
  if (!response.content?.['application/json']) return []
  const validate = ajv.compile({
    $ref: `openapi#${pointer('paths', found.specPath, method.toLowerCase(), 'responses', String(status), 'content', 'application/json', 'schema')}`,
  })
  return validate(body)
    ? []
    : (validate.errors ?? []).map((e) => `${method.toUpperCase()} ${found.specPath} ${status}：${e.instancePath || '/'} ${e.message}`)
}

/**
 * 已納入「所有 MSW 假回應都要符合契約」的測試（以測試檔路徑比對）。
 * 其餘目錄逐步加入；每加一個目錄，該目錄測試裡與 docs/api/openapi.json 不符的假回應要先修正。
 */
export const CONTRACT_ENFORCED = ['src/features/books/', 'src/features/classrooms/', 'src/features/dashboard/']
