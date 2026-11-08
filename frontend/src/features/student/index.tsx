import { TasksPage } from '@/features/tasks/tasks-page'
import { PartsPage } from '@/features/purchase/parts-page'
import type { Feature } from '@/app/feature'

// 選題頁就是首頁（/），由 app/routes.tsx 的 index route 提供，所以這裡沒有自己的路由
export const studentFeature: Feature = {
  routes: [
    { path: '/tasks', element: <TasksPage />, access: 'member' },
    { path: '/parts', element: <PartsPage />, access: 'member' },
  ],
  nav: [
    { to: '/', labelKey: 'student.nav.available', access: 'member' },
    { to: '/tasks', labelKey: 'tasks.nav', access: 'member' },
    { to: '/parts', labelKey: 'parts.nav', access: 'member' },
  ],
}
