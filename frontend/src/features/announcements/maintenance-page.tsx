import { useEffect } from 'react'
import { Link } from 'react-router'
import { useAuth } from '@/auth/auth-context'
import { AuthCard, AuthFrame, AuthHeading } from '@/components/auth-frame'
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'
import { AnnouncementBanners } from './announcement-banner'
import { useAnnouncements } from './use-announcements'

/** 維護結束後多久內會自己離開這一頁。 */
const RECHECK_MS = 30_000

/**
 * 維護模式（S-15）：後端對非管理者的請求回 503 `maintenance` 時顯示。
 * 只有固定文字與目前的公告，不含時間或原因；管理者仍可從登入頁登入。
 */
export function MaintenancePage() {
  const t = useT()
  const auth = useAuth()
  const data = useAnnouncements()

  useEffect(() => {
    const timer = setInterval(() => void auth.refresh(), RECHECK_MS)
    return () => clearInterval(timer)
  }, [auth])

  return (
    <AuthFrame>
      <div className="flex w-full max-w-[400px] flex-col gap-4">
        {data ? <AnnouncementBanners items={data.announcements} dismissible={false} /> : null}
        <AuthCard aria-labelledby="maintenance-title">
          <AuthHeading id="maintenance-title" title={t('maintenance.title')} description={t('maintenance.body')} />
          <Button size="lg" className="w-full" onClick={() => void auth.refresh()}>
            {t('maintenance.recheck')}
          </Button>
          <Link to="/login" className="text-muted-foreground text-center text-sm underline underline-offset-4">
            {t('maintenance.adminLogin')}
          </Link>
        </AuthCard>
      </div>
    </AuthFrame>
  )
}
