import type { MessageKey } from '@/i18n'
import type { Roles } from '@/lib/types'
import type { PlateId } from './plates'

export type ShellRole = 'student' | 'teacher' | 'admin'

export type BuildingId = 'basilica' | 'vesta' | 'rostra' | 'saturn' | 'castor' | 'tabularium' | 'curia'

export type HeaderWash = 'aerugo' | 'ochra' | 'caeruleum' | 'none'

export type BuildingMeta = {
  id: BuildingId
  latin: string
  plate: PlateId
  /** Focal point matching prototypes/c-interior/index.html POS & ISSUES.md §2.3. */
  pos: string
  /** Pigment wash matching DESIGN.md 頁首淡彩. */
  wash: HeaderWash
}

export const BUILDINGS: Record<BuildingId, BuildingMeta> = {
  basilica: { id: 'basilica', latin: 'Basilica Ivlia', plate: 1, pos: '58% 62%', wash: 'aerugo' },
  vesta: { id: 'vesta', latin: 'Aedes Vestae', plate: 1, pos: '92% 58%', wash: 'none' },
  rostra: { id: 'rostra', latin: 'Rostra', plate: 2, pos: '50% 55%', wash: 'caeruleum' },
  saturn: { id: 'saturn', latin: 'Aedes Satvrni', plate: 4, pos: '40% 40%', wash: 'ochra' },
  castor: { id: 'castor', latin: 'Aedes Castoris', plate: 1, pos: '78% 45%', wash: 'none' },
  tabularium: { id: 'tabularium', latin: 'Tabvlarivm', plate: 3, pos: '50% 40%', wash: 'none' },
  curia: { id: 'curia', latin: 'Cvria', plate: 4, pos: '55% 30%', wash: 'none' },
}

export type AreaSubTab = {
  to: string
  labelKey: MessageKey
}

export type ShellArea = {
  id: string
  numeral: string
  to: string
  labelKey: MessageKey
  /** Compact label used on the mobile Plaza tab when this non-frequent area is current. */
  shortLabelKey?: MessageKey
  building: BuildingId
  /** Grouped sub-pages shown as a Nav/Tab Item row under PageHeader. */
  subTabs?: AreaSubTab[]
}

export const AREAS_BY_ROLE: Record<ShellRole, ShellArea[]> = {
  student: [
    { id: 'classrooms', numeral: 'I', to: '/', labelKey: 'shell.area.student.classrooms', building: 'basilica' },
    { id: 'practice', numeral: 'II', to: '/#practice', labelKey: 'shell.area.student.practice', building: 'vesta' },
    { id: 'conversations', numeral: 'III', to: '/conversations', labelKey: 'shell.area.student.conversations', building: 'rostra' },
    {
      id: 'tasks',
      numeral: 'IV',
      to: '/tasks',
      labelKey: 'shell.area.student.tasks',
      shortLabelKey: 'shell.area.student.tasks.short',
      building: 'saturn',
    },
    {
      id: 'companion',
      numeral: 'V',
      to: '/parts',
      labelKey: 'shell.area.student.companion',
      shortLabelKey: 'shell.area.student.companion.short',
      building: 'castor',
    },
    {
      id: 'profile',
      numeral: 'VI',
      to: '/account',
      labelKey: 'shell.area.student.profile',
      shortLabelKey: 'shell.area.student.profile.short',
      building: 'tabularium',
    },
  ],
  teacher: [
    {
      id: 'classrooms',
      numeral: 'I',
      to: '/teacher/classrooms',
      labelKey: 'shell.area.teacher.classrooms',
      building: 'basilica',
      subTabs: [
        { to: '/teacher/classrooms', labelKey: 'shell.area.teacher.classrooms' },
        { to: '/teacher/keys', labelKey: 'keys.nav' },
      ],
    },
    { id: 'topics', numeral: 'II', to: '/teacher/topics', labelKey: 'shell.area.teacher.topics', building: 'tabularium' },
    {
      id: 'books',
      numeral: 'III',
      to: '/teacher/books',
      labelKey: 'shell.area.teacher.books',
      shortLabelKey: 'shell.area.teacher.books.short',
      building: 'saturn',
    },
    {
      id: 'roster',
      numeral: 'IV',
      to: '/teacher/roster',
      labelKey: 'shell.area.teacher.roster',
      shortLabelKey: 'shell.area.teacher.roster.short',
      building: 'curia',
    },
    { id: 'summaries', numeral: 'V', to: '/teacher/summaries', labelKey: 'shell.area.teacher.summaries', building: 'rostra' },
  ],
  admin: [
    { id: 'overview', numeral: 'I', to: '/admin', labelKey: 'shell.area.admin.overview', building: 'curia' },
    { id: 'classrooms', numeral: 'II', to: '/admin/classrooms', labelKey: 'shell.area.admin.classrooms', building: 'basilica' },
    {
      id: 'teachers',
      numeral: 'III',
      to: '/admin/teachers',
      labelKey: 'shell.area.admin.teachers',
      building: 'castor',
      subTabs: [
        { to: '/admin/teachers', labelKey: 'admin.nav.teachers' },
        { to: '/admin/admins', labelKey: 'admin.nav.admins' },
        { to: '/admin/teachers/keys', labelKey: 'keys.nav' },
      ],
    },
    {
      id: 'books',
      numeral: 'IV',
      to: '/admin/books',
      labelKey: 'shell.area.admin.books',
      shortLabelKey: 'shell.area.admin.books.short',
      building: 'saturn',
    },
    {
      id: 'topics',
      numeral: 'V',
      to: '/admin/topics',
      labelKey: 'shell.area.admin.topics',
      shortLabelKey: 'shell.area.admin.topics.short',
      building: 'tabularium',
    },
    {
      id: 'settings',
      numeral: 'VI',
      to: '/admin/system-ai',
      labelKey: 'shell.area.admin.settings',
      shortLabelKey: 'shell.area.admin.settings.short',
      building: 'vesta',
      subTabs: [
        { to: '/admin/system-ai', labelKey: 'systemAi.nav' },
        { to: '/admin/announcements', labelKey: 'admin.nav.announcements' },
        { to: '/admin/tasks', labelKey: 'tasks.admin' },
        { to: '/admin/parts', labelKey: 'parts.admin' },
      ],
    },
    {
      id: 'logs',
      numeral: 'VII',
      to: '/admin/diagnostics',
      labelKey: 'shell.area.admin.logs',
      shortLabelKey: 'shell.area.admin.logs.short',
      building: 'rostra',
      subTabs: [
        { to: '/admin/diagnostics', labelKey: 'admin.nav.diagnostics' },
        { to: '/admin/services', labelKey: 'admin.nav.services' },
        { to: '/admin/complaints', labelKey: 'complaints.admin.nav' },
      ],
    },
  ],
}

/** Three frequent areas shown in the mobile bottom TabBar, always in plate numeral order. */
export const FREQUENT_AREA_IDS: Record<ShellRole, readonly string[]> = {
  student: ['classrooms', 'practice', 'conversations'],
  teacher: ['classrooms', 'topics', 'summaries'],
  admin: ['overview', 'classrooms', 'teachers'],
}

export const ROLE_HOME: Record<ShellRole, string> = {
  student: '/',
  teacher: '/teacher/classrooms',
  admin: '/admin',
}

export const ROLE_FOOTNOTE_KEY: Record<ShellRole, MessageKey> = {
  student: 'shell.foot.student',
  teacher: 'shell.foot.teacher',
  admin: 'shell.foot.admin',
}

function matchesPrefix(pathname: string, prefix: string): boolean {
  return pathname === prefix || pathname.startsWith(`${prefix}/`)
}

/**
 * Resolves which role's legend to display: follows the current route path
 * (`/admin/*` → admin, `/teacher/*` → teacher, otherwise student → teacher → admin).
 */
export function resolveShellRole(pathname: string, roles: Roles): ShellRole {
  if (matchesPrefix(pathname, '/admin') && roles.admin) return 'admin'
  if (matchesPrefix(pathname, '/teacher') && roles.teacher) return 'teacher'
  if (roles.student) return 'student'
  if (roles.teacher) return 'teacher'
  return 'admin'
}

export function resolveCurrentArea(role: ShellRole, pathname: string, hash: string): ShellArea | undefined {
  const areas = AREAS_BY_ROLE[role]
  const byId = (id: string) => areas.find((a) => a.id === id)

  if (role === 'student') {
    if (pathname === '/') return hash === '#practice' ? byId('practice') : byId('classrooms')
    if (matchesPrefix(pathname, '/topics')) return byId('classrooms')
    if (matchesPrefix(pathname, '/conversations')) return byId('conversations')
    if (matchesPrefix(pathname, '/tasks')) return byId('tasks')
    if (matchesPrefix(pathname, '/parts')) return byId('companion')
    if (matchesPrefix(pathname, '/account')) return byId('profile')
    return undefined
  }

  if (role === 'teacher') {
    if (matchesPrefix(pathname, '/teacher/classrooms') || matchesPrefix(pathname, '/teacher/keys')) return byId('classrooms')
    if (matchesPrefix(pathname, '/teacher/topics')) return byId('topics')
    if (matchesPrefix(pathname, '/teacher/books')) return byId('books')
    if (matchesPrefix(pathname, '/teacher/roster')) return byId('roster')
    if (matchesPrefix(pathname, '/teacher/summaries')) return byId('summaries')
    return undefined
  }

  if (pathname === '/admin') return byId('overview')
  if (matchesPrefix(pathname, '/admin/classrooms') || matchesPrefix(pathname, '/admin/roster')) return byId('classrooms')
  if (matchesPrefix(pathname, '/admin/teachers') || matchesPrefix(pathname, '/admin/admins')) return byId('teachers')
  if (matchesPrefix(pathname, '/admin/books')) return byId('books')
  if (matchesPrefix(pathname, '/admin/topics')) return byId('topics')
  if (
    matchesPrefix(pathname, '/admin/system-ai') ||
    matchesPrefix(pathname, '/admin/announcements') ||
    matchesPrefix(pathname, '/admin/tasks') ||
    matchesPrefix(pathname, '/admin/parts')
  ) {
    return byId('settings')
  }
  if (
    matchesPrefix(pathname, '/admin/diagnostics') ||
    matchesPrefix(pathname, '/admin/services') ||
    matchesPrefix(pathname, '/admin/complaints')
  ) {
    return byId('logs')
  }
  return undefined
}

/** Shows the area's grouped sub-tabs only on its top-level sub-page routes, not deep detail pages. */
export function resolveAreaSubTabs(area: ShellArea | undefined, pathname: string): AreaSubTab[] | undefined {
  if (!area?.subTabs?.length) return undefined
  return area.subTabs.some((tab) => pathname === tab.to) ? area.subTabs : undefined
}

export type FocusConfig = {
  backTo: string
  backLabelKey: MessageKey
  titleKey: MessageKey
  numeral: string
  building: BuildingId
}

const CONVERSATION_RE = /^\/conversations\/([^/]+)$/
const TRANSCRIPT_RE = /^\/(teacher|admin)\/classrooms\/([^/]+)\/conversations\/([^/]+)$/
const STUDENT_SUMMARY_RE = /^\/(teacher|admin)\/classrooms\/([^/]+)\/students\/([^/]+)$/

/** Focus shell applies to single-document reading/writing pages: conversation, transcript, and student summary. */
export function resolveFocusConfig(pathname: string, role: ShellRole): FocusConfig | null {
  if (CONVERSATION_RE.test(pathname)) {
    return {
      backTo: '/conversations',
      backLabelKey: 'shell.focus.backToConversations',
      titleKey: 'shell.focus.conversation',
      numeral: 'III',
      building: 'rostra',
    }
  }
  const transcript = pathname.match(TRANSCRIPT_RE)
  if (transcript) {
    const [, prefix, classroomId] = transcript
    return {
      backTo: `/${prefix}/classrooms/${classroomId}`,
      backLabelKey: 'shell.focus.backToClassroom',
      titleKey: 'shell.focus.transcript',
      numeral: prefix === 'admin' || role === 'admin' ? 'VII' : 'V',
      building: 'rostra',
    }
  }
  const summary = pathname.match(STUDENT_SUMMARY_RE)
  if (summary) {
    const [, prefix, classroomId] = summary
    return {
      backTo: `/${prefix}/classrooms/${classroomId}/students`,
      backLabelKey: 'shell.focus.backToStudents',
      titleKey: 'shell.focus.summary',
      numeral: prefix === 'admin' || role === 'admin' ? 'VII' : 'V',
      building: 'rostra',
    }
  }
  return null
}

/** Other workspace roles a multi-role user can switch into via Nav/Account Menu. */
export function availableSwitchRoles(roles: Roles, current: ShellRole): ShellRole[] {
  const all: ShellRole[] = []
  if (roles.student) all.push('student')
  if (roles.teacher) all.push('teacher')
  if (roles.admin) all.push('admin')
  return all.filter((r) => r !== current)
}
