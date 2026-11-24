import { setupServer } from 'msw/node'
import { defaultHandlers } from '@/test/handlers'

/** 測試用的假後端（MSW）。各測試以 `server.use(...)` 加上自己需要的端點。 */
export const server = setupServer(...defaultHandlers)
