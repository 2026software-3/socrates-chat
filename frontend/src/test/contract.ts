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

export function findOperation(method: string, path: string) {
  const wanted = normalize(path)
  const specPath = Object.keys(paths).find((p) => normalize(p) === wanted)
  const operation = specPath ? paths[specPath][method.toLowerCase()] : undefined
  return specPath && operation ? { specPath, operation } : undefined
}

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
