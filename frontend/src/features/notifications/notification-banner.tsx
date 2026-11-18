import { X } from 'lucide-react'
import { useState } from 'react'
import { ErrorBlock } from '@/components/page'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { useI18n } from '@/i18n'
import { api } from '@/lib/api'
import { useFetch } from '@/lib/use-fetch'

type StudentNotification = {
  id: string
  kind: 'student_number_corrected' | 'teacher_api_key_deleted'
  created_at: string
}

export function NotificationBanner() {
  const { t, lang } = useI18n()
  const { data, error, reload } = useFetch<StudentNotification[]>('/api/notifications')
  const [dismissed, setDismissed] = useState<string[]>([])
  const [busy, setBusy] = useState(false)
  const [dismissError, setDismissError] = useState<unknown>()
  const visible = data?.filter((notice) => !dismissed.includes(notice.id)) ?? []

  async function dismiss(id: string) {
    if (busy) return
    setBusy(true)
    setDismissError(undefined)
    try {
      await api.post(`/api/notifications/${encodeURIComponent(id)}/read`)
      setDismissed((previous) => [...previous, id])
    } catch (err) {
      setDismissError(err)
    } finally {
      setBusy(false)
    }
  }

  if (!error && visible.length === 0) return null
  const timeFormat = new Intl.DateTimeFormat(lang, { dateStyle: 'medium', timeStyle: 'short' })
  return (
    <section aria-label={t('notifications.region')} className="flex flex-col gap-2">
      {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
      {dismissError ? <ErrorBlock error={dismissError} /> : null}
      {visible.map((notice) => (
        <Alert key={notice.id} role="status" className="flex items-start justify-between gap-3">
          <AlertDescription className="text-foreground">
            <p>{t(`notifications.${notice.kind}`)}</p>
            <time dateTime={notice.created_at} className="text-muted-foreground">
              {timeFormat.format(new Date(notice.created_at))}
            </time>
          </AlertDescription>
          <Button variant="ghost" size="icon-sm" className="-my-1 -mr-1 shrink-0" disabled={busy} aria-label={t('notifications.dismiss')} onClick={() => void dismiss(notice.id)}>
            <X aria-hidden />
          </Button>
        </Alert>
      ))}
    </section>
  )
}
