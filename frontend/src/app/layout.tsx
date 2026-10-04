import { NavLink, Link } from 'react-router'
import type { ReactNode } from 'react'
import { useAuth } from '@/auth/auth-context'
import { canAccess, type NavItem } from '@/app/feature'
import { LangSwitcher } from '@/components/lang-switcher'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'
import type { Me } from '@/lib/types'

/** 已登入後的外框：標題、依角色顯示的導覽、語言切換與登出。手機寬度時導覽可橫向捲動。 */
export function Layout({ me, nav, children }: { me: Me; nav: NavItem[]; children: ReactNode }) {
  const t = useT()
  const { logout } = useAuth()
  const items = nav.filter((n) => canAccess(n.access, me.roles))
  const roleBadges = (['admin', 'teacher', 'student'] as const).filter((r) => me.roles[r])

  return (
    <div className="min-h-svh">
      <header className="border-b">
        <div className="mx-auto flex max-w-5xl flex-wrap items-center gap-x-4 gap-y-2 px-4 py-3">
          <Link to="/" className="font-semibold">
            {t('app.name')}
          </Link>
          <div className="ml-auto flex flex-wrap items-center gap-3">
            <span className="text-sm">{me.display_name ?? me.email}</span>
            {roleBadges.map((r) => (
              <Badge key={r} variant="secondary">
                {t(`role.${r}`)}
              </Badge>
            ))}
            <LangSwitcher />
            <Button variant="outline" size="sm" onClick={() => void logout()}>
              {t('nav.logout')}
            </Button>
          </div>
        </div>
        {items.length > 0 ? (
          <nav aria-label="main" className="mx-auto flex max-w-5xl gap-1 overflow-x-auto px-4 pb-2">
            {items.map((n) => (
              <NavLink
                key={n.to}
                to={n.to}
                end={n.to === '/'}
                className={({ isActive }) =>
                  `rounded-md px-3 py-1.5 text-sm whitespace-nowrap ${isActive ? 'bg-secondary font-medium' : 'text-muted-foreground hover:bg-muted'}`
                }
              >
                {t(n.labelKey)}
              </NavLink>
            ))}
          </nav>
        ) : null}
      </header>
      <main className="mx-auto max-w-5xl p-4">{children}</main>
    </div>
  )
}
