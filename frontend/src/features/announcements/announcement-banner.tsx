import { X } from 'lucide-react'
import { useState } from 'react'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'
import { cn } from '@/lib/utils'
import { useAnnouncements, type PublicAnnouncement } from './use-announcements'

const DISMISSED_KEY = 'dismissed-announcements'

/** 使用者關閉過的公告編號（只存在這個瀏覽器；公告換了編號就會再顯示）。 */
function readDismissed(): string[] {
  try {
    const raw = localStorage.getItem(DISMISSED_KEY)
    const ids: unknown = raw ? JSON.parse(raw) : []
    return Array.isArray(ids) ? ids.filter((x): x is string => typeof x === 'string') : []
  } catch {
    return []
  }
}

function rememberDismissed(ids: string[]) {
  try {
    localStorage.setItem(DISMISSED_KEY, JSON.stringify(ids))
  } catch {
    // localStorage 不可用（隱私模式等）：只在這次頁面內關閉
  }
}

/** 公告橫幅清單。`dismissible` 為 false 時（維護頁）沒有關閉鈕。 */
export function AnnouncementBanners({ items, dismissible = true }: { items: PublicAnnouncement[]; dismissible?: boolean }) {
  const t = useT()
  const [dismissed, setDismissed] = useState(readDismissed)
  const visible = dismissible ? items.filter((a) => !dismissed.includes(a.id)) : items
  if (visible.length === 0) return null

  function dismiss(id: string) {
    // 只保留仍存在的公告編號，避免清單無限成長
    const next = [...dismissed.filter((d) => items.some((a) => a.id === d)), id]
    setDismissed(next)
    rememberDismissed(next)
  }

  return (
    <section aria-label={t('announcements.region')} className="flex flex-col gap-2">
      {visible.map((a) => (
        <Alert
          key={a.id}
          role="status"
          className={cn('flex items-start justify-between gap-3', a.kind === 'downtime' && 'border-amber-500/60 bg-amber-50 dark:bg-amber-950/30')}
        >
          <AlertDescription className="text-foreground min-w-0 whitespace-pre-line break-words">{a.text}</AlertDescription>
          {dismissible ? (
            <Button variant="ghost" size="icon-sm" className="-my-1 -mr-1 shrink-0" aria-label={t('announcements.dismiss')} onClick={() => dismiss(a.id)}>
              <X aria-hidden />
            </Button>
          ) : null}
        </Alert>
      ))}
    </section>
  )
}

/** 自己讀取公告的橫幅（登入後外框用）。 */
export function AnnouncementBar() {
  const data = useAnnouncements()
  return data ? <AnnouncementBanners items={data.announcements} /> : null
}
