import { Link, Navigate, Outlet, Route, Routes } from 'react-router'
import { hasAnyRole, useAuth } from '@/auth/auth-context'
import { canAccess, type Access, type Feature } from '@/app/feature'
import { Layout } from '@/app/layout'
import { ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Card, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { adminFeature } from '@/features/admin'
import { LoginPage } from '@/features/auth/login-page'
import { ChangePasswordPage } from '@/features/auth/change-password-page'
import { PendingPage } from '@/features/auth/pending-page'
import { chatFeature } from '@/features/chat'
import { studentFeature } from '@/features/student'
import { teacherFeature } from '@/features/teacher'
import { useT } from '@/i18n'
import type { ReactNode } from 'react'

// 新增功能時：在 features/<name>/index.tsx 匯出 Feature，再加進這裡。
export const features: Feature[] = [studentFeature, chatFeature, teacherFeature, adminFeature]
const allNav = features.flatMap((f) => f.nav)

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
    <Layout me={me} nav={hasAnyRole(me.roles) ? allNav : []}>
      {hasAnyRole(me.roles) ? <Outlet /> : <PendingPage />}
    </Layout>
  )
}

function RequireAccess({ access, children }: { access: Access; children: ReactNode }) {
  const auth = useAuth()
  const t = useT()
  if (auth.status !== 'authed') return null
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

function HomePage() {
  const t = useT()
  const auth = useAuth()
  if (auth.status !== 'authed') return null
  const { me } = auth
  const items = allNav.filter((n) => canAccess(n.access, me.roles))
  return (
    <Page title={t('home.welcome', { name: me.display_name ?? me.email })}>
      <p className="text-muted-foreground">{t('home.choose')}</p>
      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
        {items.map((n) => (
          <Link key={n.to} to={n.to}>
            <Card className="hover:bg-muted/50 h-full transition-colors">
              <CardHeader>
                <CardTitle>{t(n.labelKey)}</CardTitle>
                <CardDescription>{n.to}</CardDescription>
              </CardHeader>
            </Card>
          </Link>
        ))}
      </div>
    </Page>
  )
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
        <Route index element={<HomePage />} />
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
