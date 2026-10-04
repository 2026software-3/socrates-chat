import { Link, Navigate, Outlet, Route, Routes, useLocation } from 'react-router'
import { hasAnyRole, useAuth } from '@/auth/auth-context'
import { canAccess, type Access, type Feature } from '@/app/feature'
import { Layout } from '@/app/layout'
import { ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { adminFeature, opsNav } from '@/features/admin'
import { DisabledPage } from '@/features/auth/disabled-page'
import { LoginPage } from '@/features/auth/login-page'
import { ChangePasswordPage } from '@/features/auth/change-password-page'
import { PendingPage } from '@/features/auth/pending-page'
import { chatFeature } from '@/features/chat'
import { dashboardFeature } from '@/features/dashboard'
import { studentFeature } from '@/features/student'
import { AvailablePage } from '@/features/student/available-page'
import { teacherFeature } from '@/features/teacher'
import { useT } from '@/i18n'
import type { Roles } from '@/lib/types'
import type { ReactNode } from 'react'

// 新增功能時：在 features/<name>/index.tsx 匯出 Feature，再加進這裡。
export const features: Feature[] = [studentFeature, chatFeature, dashboardFeature, teacherFeature, adminFeature]
const allNav = features.flatMap((f) => f.nav)
/** 不參與討論的教師：拿掉學生功能（選題、我的對話、我的儀表板）。 */
const staffNav = allNav.filter((n) => n.access !== 'member')

/** 純管理者：只有管理者角色，只做系統維運，不參與討論。 */
const isOpsAdmin = (roles: Roles) => roles.admin && !roles.teacher && !roles.student

/** 教師與管理者不參與討論（不選題、不對話）：只有學生（修課名單內）才有討論相關功能。 */
const participates = (roles: Roles) => roles.student

/** 已登入外框：未登入導向 /login；沒有任何角色（尚未開通）只看得到說明頁。 */
function Shell() {
  const auth = useAuth()
  if (auth.status === 'loading') {
    return (
      <div className="p-4">
        <LoadingBlock />
      </div>
    )
  }
  if (auth.status === 'error') {
    return (
      <div className="p-4">
        <ErrorBlock error={undefined} onRetry={() => void auth.refresh()} />
      </div>
    )
  }
  if (auth.status === 'anon') return <Navigate to="/login" replace />
  if (auth.status === 'disabled') return <DisabledPage />
  const { me } = auth
  // 臨時密碼：改密碼前其他 API 都會回 password_change_required，所以只放行改密碼頁
  if (me.must_change_password) {
    return (
      <Layout me={me} nav={[]}>
        <ChangePasswordPage />
      </Layout>
    )
  }
  return (
    <Layout me={me} nav={isOpsAdmin(me.roles) ? opsNav : hasAnyRole(me.roles) ? (participates(me.roles) ? allNav : staffNav) : []}>
      {hasAnyRole(me.roles) ? <Outlet /> : <PendingPage />}
    </Layout>
  )
}

function RequireAccess({ access, children }: { access: Access; children: ReactNode }) {
  const auth = useAuth()
  const t = useT()
  const { pathname } = useLocation()
  if (auth.status !== 'authed') return null
  // 純管理者只能進維運頁面；其他功能頁導回維運首頁
  if (isOpsAdmin(auth.me.roles) && !pathname.startsWith('/admin')) return <Navigate to="/admin" replace />
  // 教師不參與討論：學生功能導向教師首頁
  if (access === 'member' && !participates(auth.me.roles)) return <Navigate to="/teacher/dashboard" replace />
  if (!canAccess(access, auth.me.roles)) {
    return (
      <Page title={t('error.forbidden')}>
        <Link to="/" className="underline">
          {t('nav.home')}
        </Link>
      </Page>
    )
  }
  return <>{children}</>
}

/** 首頁：純管理者進維運首頁，教師進班上分布，學生直接進選題。 */
function Home() {
  const auth = useAuth()
  if (auth.status === 'authed' && isOpsAdmin(auth.me.roles)) return <Navigate to="/admin" replace />
  if (auth.status === 'authed' && !participates(auth.me.roles)) return <Navigate to="/teacher/dashboard" replace />
  return <AvailablePage />
}

function NotFoundPage() {
  const t = useT()
  return (
    <Page title={t('error.not_found')}>
      <Link to="/" className="underline">
        {t('nav.home')}
      </Link>
    </Page>
  )
}

export function AppRoutes() {
  return (
    <Routes>
      <Route path="/login" element={<LoginPage />} />
      <Route element={<Shell />}>
        <Route index element={<Home />} />
        {features.flatMap((f) =>
          f.routes.map((r) => (
            <Route key={r.path} path={r.path} element={<RequireAccess access={r.access}>{r.element}</RequireAccess>} />
          )),
        )}
        <Route path="*" element={<NotFoundPage />} />
      </Route>
    </Routes>
  )
}
