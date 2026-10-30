import { useLocation } from 'react-router'

/** 教室相關頁面的網址前綴：管理者工作區在 `/admin`，教師在 `/teacher`。 */
export function useClassroomsBase(): string {
  const { pathname } = useLocation()
  return pathname.startsWith('/admin') ? '/admin/classrooms' : '/teacher/classrooms'
}
