import type { Feature } from '@/app/feature'
import { ProfilePage } from '@/features/student/profile-page'

// 選題頁就是首頁（/），由 app/routes.tsx 的 index route 提供，所以這裡沒有自己的導覽以外的路由；
// 個人設定從頂部列的名稱進入，不放在導覽列。
export const studentFeature: Feature = {
  routes: [{ path: '/settings', element: <ProfilePage />, access: 'member' }],
  nav: [{ to: '/', labelKey: 'student.nav.available', access: 'member' }],
}
