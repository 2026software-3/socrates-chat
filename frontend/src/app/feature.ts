import type { ReactNode } from 'react'
import type { MessageKey } from '@/i18n'
import type { Roles } from '@/lib/types'

/**
 * 路由的存取層級：
 * - `member`：學生、教師或管理者（學生功能；名單外帳號看到「尚未開通」）
 * - `teacher`：教師或管理者
 * - `admin`：管理者
 */
export type Access = 'member' | 'teacher' | 'admin'

export type AppRoute = { path: string; element: ReactNode; access: Access }
export type NavItem = { to: string; labelKey: MessageKey; access: Access }

/** 每個功能目錄（features/*）匯出一個 Feature，由 app/routes.tsx 彙整。 */
export type Feature = { routes: AppRoute[]; nav: NavItem[] }

export function canAccess(access: Access, roles: Roles): boolean {
  switch (access) {
    case 'member':
      return roles.admin || roles.teacher || roles.student
    case 'teacher':
      return roles.admin || roles.teacher
    case 'admin':
      return roles.admin
  }
}
