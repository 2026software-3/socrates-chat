import { lazyPage } from '@/app/lazy-page'

// 教師與管理者頁面的延遲載入版本，供各功能的路由表使用；頁面本身在 features/staff-pages.ts。
const load = () => import('@/features/staff-pages')

export const AdminHome = lazyPage(load, 'AdminHome')
export const DiagnosticsPage = lazyPage(load, 'DiagnosticsPage')
export const TeachersPage = lazyPage(load, 'TeachersPage')
export const TopicsPage = lazyPage(load, 'TopicsPage')
export const AdminAnnouncementsPage = lazyPage(load, 'AdminAnnouncementsPage')
export const ClassroomActivityPage = lazyPage(load, 'ClassroomActivityPage')
export const ClassroomPage = lazyPage(load, 'ClassroomPage')
export const ClassroomStudentPage = lazyPage(load, 'ClassroomStudentPage')
export const ClassroomStudentsPage = lazyPage(load, 'ClassroomStudentsPage')
export const ClassroomsPage = lazyPage(load, 'ClassroomsPage')
export const TranscriptPage = lazyPage(load, 'TranscriptPage')
export const AdminComplaintsPage = lazyPage(load, 'AdminComplaintsPage')
export const AdminPartsPage = lazyPage(load, 'AdminPartsPage')
export const AdminTasksPage = lazyPage(load, 'AdminTasksPage')
export const RosterPage = lazyPage(load, 'RosterPage')
export const SummariesPage = lazyPage(load, 'SummariesPage')
