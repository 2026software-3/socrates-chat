import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { useLocation } from 'react-router'
import { toast } from 'sonner'
import { useAuth } from '@/auth/auth-context'
import type { NavItem } from '@/app/feature'
import {
  AREAS_BY_ROLE,
  AreaSubTabs,
  BUILDINGS,
  BackHeader,
  Colophon,
  PageHeader as VedutePageHeader,
  PlanSheet,
  ShellHeaderContext,
  Sidebar,
  SidebarDrawer,
  TabBar,
  Topbar,
  resolveAreaSubTabs,
  resolveCurrentArea,
  resolveFocusConfig,
  resolveShellRole,
  type ShellHeaderContextValue,
} from '@/components/vedute/shell'
import { WorkspaceContext } from '@/components/page'
import { AiJobsIndicator } from '@/features/jobs/ai-jobs-indicator'
import { AnnouncementBar } from '@/features/announcements/announcement-banner'
import { NotificationBanner } from '@/features/notifications/notification-banner'
import { AiUsageMeter } from '@/features/usage/ai-usage-meter'
import { useMediaQuery } from '@/hooks/use-media-query'
import { errorText, useI18n } from '@/i18n'
import type { Lang } from '@/i18n/define'
import { api } from '@/lib/api'
import type { Me } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

function useStudentPoints(enabled: boolean): number | null {
  const { data, error } = useFetch<{ balance: number }>(enabled ? '/api/points' : null)
  if (!enabled || error || !data) return null
  return data.balance
}

/**
 * Vedute 工作情境與專注情境殼層（F-41.4，取代舊 AppShell）。
 * - 工作情境：Topbar + 側欄（≥1024 常駐 / 768–1023 抽屜 / <768 底部分頁列＋平面圖面板）+ PageHeader + 白色內容區 + Colophon。
 * - 專注情境（對話、對話原文、學生總結）：BackHeader + 置中內容區，無側欄、分頁列與頁尾。
 */
export function Layout({ me, children }: { me: Me; nav?: NavItem[]; children: ReactNode }) {
  const { lang, setLang, t } = useI18n()
  const auth = useAuth()
  const { pathname, hash } = useLocation()

  const role = resolveShellRole(pathname, me.roles)
  const areas = AREAS_BY_ROLE[role]
  const currentArea = resolveCurrentArea(role, pathname, hash)
  const subTabs = resolveAreaSubTabs(currentArea, pathname)
  const focusConfig = resolveFocusConfig(pathname, role)

  const locationKey = `${pathname}${hash}`
  const isMobile = useMediaQuery('(max-width: 767px)', true)
  const isTablet = useMediaQuery('(min-width: 768px) and (max-width: 1023px)', true)
  const [drawerAt, setDrawerAt] = useState<string | null>(null)
  const [sheetAt, setSheetAt] = useState<string | null>(null)
  const drawerOpen = drawerAt === locationKey && isTablet
  const sheetOpen = sheetAt === locationKey && isMobile
  const [savingLang, setSavingLang] = useState(false)
  const [headerSlot, setHeaderSlot] = useState<HTMLDivElement | null>(null)
  const [pageHeaderCount, setPageHeaderCount] = useState(0)
  const [focusTitle, setFocusTitle] = useState<string | null>(null)
  const [focusDetail, setFocusDetail] = useState<string | null>(null)

  // 跨斷點（如手機旋轉成平板、平板拉寬成桌機）時清除對應面板狀態，避免隱藏後的 focus trap 繼續攔截鍵盤
  useEffect(() => {
    if (typeof window.matchMedia !== 'function') return
    const mqMobile = window.matchMedia('(max-width: 767px)')
    const mqTablet = window.matchMedia('(min-width: 768px) and (max-width: 1023px)')
    const onMobileChange = () => {
      if (!mqMobile.matches) setSheetAt(null)
    }
    const onTabletChange = () => {
      if (!mqTablet.matches) setDrawerAt(null)
    }
    mqMobile.addEventListener('change', onMobileChange)
    mqTablet.addEventListener('change', onTabletChange)
    return () => {
      mqMobile.removeEventListener('change', onMobileChange)
      mqTablet.removeEventListener('change', onTabletChange)
    }
  }, [])

  const menuButtonRef = useRef<HTMLButtonElement>(null)
  const sheetTriggerRef = useRef<HTMLElement | null>(null)

  const points = useStudentPoints(role === 'student' && focusConfig === null)

  const handleOpenDrawer = useCallback(() => {
    setDrawerAt(locationKey)
  }, [locationKey])

  const handleCloseDrawer = useCallback(() => {
    setDrawerAt(null)
  }, [])

  const handleOpenSheet = useCallback(
    (trigger: HTMLElement | null) => {
      sheetTriggerRef.current = trigger
      setSheetAt(locationKey)
    },
    [locationKey],
  )

  const handleCloseSheet = useCallback(() => {
    setSheetAt(null)
  }, [])

  const registerPageHeader = useCallback(() => {
    setPageHeaderCount((c) => c + 1)
    return () => setPageHeaderCount((c) => Math.max(0, c - 1))
  }, [])

  const handleSelectLanguage = useCallback(
    async (next: Lang) => {
      const previous = lang
      setLang(next)
      setSavingLang(true)
      try {
        const updated = await api.patch<Me>('/api/me', { language: next })
        auth.update(updated)
      } catch (error) {
        setLang(previous)
        toast.error(errorText(t, error))
      } finally {
        setSavingLang(false)
      }
    },
    [auth, lang, setLang, t],
  )

  const workspace =
    role === 'student' ? null : t(role === 'admin' ? 'nav.workspace.admin' : 'nav.workspace.teacher')

  const shellHeaderValue = useMemo<ShellHeaderContextValue>(
    () => ({
      mode: focusConfig ? 'focus' : 'work',
      area: currentArea,
      headerSlot,
      registerPageHeader,
      setFocusTitle,
      setFocusDetail,
    }),
    [focusConfig, currentArea, headerSlot, registerPageHeader],
  )

  const showNotifications = me.roles.student || me.roles.teacher
  const showReportLink = me.roles.student || me.roles.teacher

  if (focusConfig) {
    const focusBuilding = BUILDINGS[focusConfig.building]
    const isConversation = pathname.startsWith('/conversations/')
    return (
      <WorkspaceContext.Provider value={workspace}>
        <ShellHeaderContext.Provider value={shellHeaderValue}>
          <div className="flex min-h-svh flex-col bg-surface">
            <BackHeader
              backTo={focusConfig.backTo}
              backLabel={t(focusConfig.backLabelKey)}
              title={focusTitle ?? t(focusConfig.titleKey)}
              numeral={focusConfig.numeral}
              latin={focusBuilding.latin}
              detail={focusDetail ?? t('shell.focus.rostraDetail')}
            />
            <AiJobsIndicator />
            <div className="mx-auto flex w-full max-w-[760px] flex-col gap-2 px-4 pt-3 empty:hidden">
              <AnnouncementBar />
              {showNotifications ? <NotificationBanner key={me.id} /> : null}
            </div>
            <main
              className={
                isConversation
                  ? 'mx-auto flex min-h-0 w-full max-w-[760px] flex-1 flex-col'
                  : 'mx-auto w-full max-w-[760px] flex-1 px-4 py-6 md:px-6 md:py-8'
              }
            >
              {children}
            </main>
          </div>
        </ShellHeaderContext.Provider>
      </WorkspaceContext.Provider>
    )
  }

  const sidebarExtra = (
    <div className="w-full">
      <AiUsageMeter />
    </div>
  )

  return (
    <WorkspaceContext.Provider value={workspace}>
      <ShellHeaderContext.Provider value={shellHeaderValue}>
        <div className="flex min-h-svh flex-col bg-paper">
          <Topbar
            me={me}
            role={role}
            area={currentArea}
            points={points}
            drawerOpen={drawerOpen}
            sheetOpen={sheetOpen}
            onOpenDrawer={handleOpenDrawer}
            onOpenSheet={handleOpenSheet}
            menuButtonRef={menuButtonRef}
            onSelectLanguage={(next) => void handleSelectLanguage(next)}
            languageDisabled={savingLang}
            onLogout={() => void auth.logout()}
          />

          <div className="flex min-h-0 flex-1">
            {/* Desktop (≥1024): persistent 216px sidebar */}
            <div className="vedute-sidebar-fixed hidden shrink-0 border-r border-ink-faint bg-paper lg:block">
              <aside className="sticky top-13 h-[calc(100svh-52px)] overflow-y-auto">
                <Sidebar
                  role={role}
                  areas={areas}
                  currentArea={currentArea}
                  footerExtra={sidebarExtra}
                  className="border-r-0"
                />
              </aside>
            </div>

            {/* Tablet (768–1023): slide-in drawer */}
            <SidebarDrawer
              open={drawerOpen}
              onClose={handleCloseDrawer}
              returnFocusTo={menuButtonRef}
              role={role}
              areas={areas}
              currentArea={currentArea}
              footerExtra={sidebarExtra}
            />

            {/* Work area (white surface + full-bleed PageHeader + optional area sub-tabs + content + Colophon) */}
            <div className="vedute-work-main flex min-w-0 flex-1 flex-col bg-surface">
              <main className="flex min-w-0 flex-1 flex-col">
                <AiJobsIndicator />
                <div ref={setHeaderSlot} data-vedute-header-slot="" className="empty:hidden" />
                {pageHeaderCount === 0 && currentArea && (
                  <VedutePageHeader
                    title={t(currentArea.labelKey)}
                    area={currentArea}
                    asHeading={false}
                  />
                )}

                {subTabs && <AreaSubTabs tabs={subTabs} pathname={pathname} />}

                <div className="flex w-full flex-col gap-2 px-4 pt-4 empty:hidden md:px-7 lg:px-10">
                  <AnnouncementBar />
                  {showNotifications ? <NotificationBanner key={me.id} /> : null}
                </div>

                <div className="w-full flex-1 px-4 py-6 md:px-7 md:py-8 lg:px-10">
                  {children}
                </div>
              </main>

              <Colophon showReportLink={showReportLink} />
            </div>
          </div>

          {/* Mobile (<768): bottom TabBar + PlanSheet */}
          <TabBar
            role={role}
            areas={areas}
            currentArea={currentArea}
            sheetOpen={sheetOpen}
            onOpenSheet={handleOpenSheet}
          />
          <PlanSheet
            open={sheetOpen}
            role={role}
            areas={areas}
            currentArea={currentArea}
            onClose={handleCloseSheet}
            returnFocusTo={sheetTriggerRef}
          />
        </div>
      </ShellHeaderContext.Provider>
    </WorkspaceContext.Provider>
  )
}
