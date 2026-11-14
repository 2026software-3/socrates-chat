// 教師與管理者的頁面集中成一個 chunk（由 features/staff.ts 延遲載入）：學生不必下載，
// 教師與管理者第一次進入任一頁時一次載入，之後換頁不再等待。
export { AdminHome } from '@/features/admin/admin-home'
export { DiagnosticsPage } from '@/features/admin/diagnostics-page'
export { TeachersPage } from '@/features/admin/teachers-page'
export { TopicsPage } from '@/features/admin/topics-page'
export { AdminAnnouncementsPage } from '@/features/announcements/admin-announcements-page'
export {
  ClassroomActivityPage,
  ClassroomPage,
  ClassroomStudentPage,
  ClassroomStudentsPage,
} from '@/features/classrooms/classroom-pages'
export { ClassroomsPage } from '@/features/classrooms/classrooms-page'
export { TranscriptPage } from '@/features/classrooms/transcript-page'
export { AdminComplaintsPage } from '@/features/complaints/admin-complaints-page'
export { AdminPartsPage } from '@/features/purchase/admin-parts-page'
export { AdminTasksPage } from '@/features/tasks/admin-tasks-page'
export { RosterPage } from '@/features/teacher/roster-page'
export { SummariesPage } from '@/features/teacher/summaries-page'
