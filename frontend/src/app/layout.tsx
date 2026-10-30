import { NavLink, Link, useMatch } from 'react-router'
import type { ReactNode } from 'react'
import { useAuth } from '@/auth/auth-context'
import { canAccess, type NavItem } from '@/app/feature'
import { LangSwitcher } from '@/components/lang-switcher'
import { WorkspaceContext } from '@/components/page'
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'
import { cn } from '@/lib/utils'
import type { Me } from '@/lib/types'

/** 學生外框用左側欄（272px）；教師與管理者用頂欄（52px）加工作區側欄（216px）。 */
const STUDENT_SIDEBAR = 'md:w-[272px]'
const WORKSPACE_SIDEBAR = 'md:w-[216px]'
/** 桌面寬度時側欄固定在視窗內，名稱、語言與登出在每一頁都貼著左下角，不會隨長頁面落到頁尾。 */
const STUDENT_STICKY = 'md:sticky md:top-0 md:h-svh md:overflow-y-auto'
const WORKSPACE_STICKY = 'md:sticky md:top-[52px] md:h-[calc(100svh-52px)] md:overflow-y-auto'

function MainNav({ items, label }: { items: NavItem[]; label: string }) {
  const t = useT()
  if (items.length === 0) return null
  return (
    <nav aria-label={label} className="flex gap-1 overflow-x-auto md:flex-col md:overflow-visible">
      {items.map((n) => (
        <NavLink
          key={n.to}
          to={n.to}
          end={n.to === '/'}
          className={({ isActive }) =>
            cn(
              'flex h-10 shrink-0 items-center rounded-lg border border-transparent px-2.5 text-sm whitespace-nowrap',
              isActive ? 'bg-background border-border text-foreground font-medium' : 'text-muted-foreground hover:bg-subtle hover:text-foreground',
            )
          }
        >
          {t(n.labelKey)}
        </NavLink>
      ))}
    </nav>
  )
}

/** 側欄底部：顯示名稱、角色、語言與登出。 */
function Account({ me }: { me: Me }) {
  const t = useT()
  const { logout } = useAuth()
  const roles = (['admin', 'teacher', 'student'] as const).filter((r) => me.roles[r])
  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1 px-2.5 md:mt-auto md:flex-col md:items-start">
      {me.roles.student ? (
        <Link to="/dashboard" className="text-sm font-medium underline-offset-4 hover:underline">
          {me.display_name ?? me.email}
        </Link>
      ) : (
        <span className="text-sm font-medium">{me.display_name ?? me.email}</span>
      )}
      <span className="text-muted-foreground hidden text-xs md:inline">{roles.map((r) => t(`role.${r}`)).join(' / ')}</span>
      <div className="flex flex-wrap items-center gap-2 md:mt-2">
        <LangSwitcher />
        <Button variant="outline" size="sm" onClick={() => void logout()}>
          {t('nav.logout')}
        </Button>
      </div>
    </div>
  )
}

/** 已登入後的外框。手機寬度時側欄移到上方，導覽可橫向捲動。 */
export function Layout({ me, nav, children }: { me: Me; nav: NavItem[]; children: ReactNode }) {
  const t = useT()
  const items = nav.filter((n) => canAccess(n.access, me.roles))
  // 對話頁撐滿內容區（標題列、對話串、輸入列各自帶邊線與內距）
  const fullBleed = useMatch('/conversations/:id') !== null
  const student = me.roles.student
  const adminOnly = me.roles.admin && !me.roles.teacher && !me.roles.student
  const displayName = me.display_name ?? me.email ?? ''
  const workspace = student ? null : t(adminOnly ? 'nav.workspace.admin' : 'nav.workspace.teacher')

  const content = (
    <WorkspaceContext.Provider value={workspace}>
      <div className={cn('flex min-w-0 flex-1 flex-col', fullBleed && 'min-h-0 overflow-y-auto')}>
        {fullBleed ? <main className="flex min-h-0 flex-1 flex-col">{children}</main> : <main className="mx-auto w-full max-w-5xl p-4 md:p-8">{children}</main>}
      </div>
    </WorkspaceContext.Provider>
  )

  if (student) {
    return (
      <div className={cn('flex flex-col md:flex-row', fullBleed ? 'h-svh' : 'min-h-svh')}>
        <aside className={cn('bg-surface flex shrink-0 flex-col gap-2 border-b p-3 md:border-r md:border-b-0 md:px-3 md:py-4', STUDENT_SIDEBAR, STUDENT_STICKY)}>
          <Link to="/" className="px-2.5 pt-2 pb-2 text-lg leading-7 font-medium md:pb-4">
            {t('app.name')}
          </Link>
          <MainNav items={items} label="main" />
          <Account me={me} />
        </aside>
        {content}
      </div>
    )
  }

  return (
    <div className="flex min-h-svh flex-col">
      <header className="bg-background sticky top-0 z-10 flex h-[52px] shrink-0 items-center justify-between border-b px-6">
        <Link to="/" className="flex items-baseline gap-3">
          <span className="text-lg leading-7 font-medium">{t('app.brand')}</span>
          <span className="text-muted-foreground text-xs">{workspace}</span>
        </Link>
        <span
          role="img"
          aria-label={displayName}
          className="bg-outline-border flex size-8 items-center justify-center rounded-full border text-sm font-medium"
        >
          {[...displayName][0]?.toUpperCase()}
        </span>
      </header>
      <div className="flex min-h-0 flex-1 flex-col md:flex-row">
        <aside className={cn('bg-surface flex shrink-0 flex-col gap-2 border-b p-3 md:border-r md:border-b-0 md:py-4', WORKSPACE_SIDEBAR, WORKSPACE_STICKY)}>
          <MainNav items={items} label="main" />
          <Account me={me} />
        </aside>
        {content}
      </div>
    </div>
  )
}
