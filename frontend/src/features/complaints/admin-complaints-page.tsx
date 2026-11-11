import { useMemo, useState } from 'react'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { useI18n, useT } from '@/i18n'
import { api } from '@/lib/api'
import { useFetch } from '@/lib/use-fetch'
import type { Category } from './complaint-page'

type Status = 'open' | 'resolved'
type MailStatus = 'pending' | 'sent' | 'failed' | 'skipped'

type Complaint = {
  id: string
  category: Category
  /** 描述原文：S-02.2 的例外，管理者看得到；日誌不記。 */
  description: string
  conversation_id: string | null
  created_at: string
  status: Status
  mail_status: MailStatus
  submitter_name: string | null
  submitter_role: 'student' | 'teacher'
  classroom_name: string | null
}

type Filter = 'all' | Status
const FILTERS: Filter[] = ['all', 'open', 'resolved']

/** 管理者的客訴列表（S-17.4）：看原文、標記處理狀態、重寄失敗的通知信。 */
export function AdminComplaintsPage() {
  const t = useT()
  const { lang } = useI18n()
  const [filter, setFilter] = useState<Filter>('all')
  const { data, loading, error, reload } = useFetch<Complaint[]>(
    `/api/admin/complaints${filter === 'all' ? '' : `?status=${filter}`}`,
  )
  const [busy, setBusy] = useState<string>()
  const [actionError, setActionError] = useState<unknown>()
  const date = useMemo(() => new Intl.DateTimeFormat(lang, { dateStyle: 'short', timeStyle: 'short' }), [lang])

  async function act(id: string, run: () => Promise<unknown>) {
    if (busy) return
    setBusy(id)
    setActionError(undefined)
    try {
      await run()
    } catch (err) {
      setActionError(err)
    } finally {
      setBusy(undefined)
      reload()
    }
  }

  return (
    <Page title={t('complaints.admin.title')}>
      <div role="group" aria-label={t('complaints.admin.filter')} className="flex flex-wrap gap-2">
        {FILTERS.map((f) => (
          <Button key={f} variant={filter === f ? 'default' : 'outline'} size="sm" aria-pressed={filter === f} onClick={() => setFilter(f)}>
            {t(`complaints.admin.${f}`)}
          </Button>
        ))}
      </div>
      {loading && !data ? <LoadingBlock /> : null}
      {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
      {actionError ? <ErrorBlock error={actionError} onRetry={() => setActionError(undefined)} /> : null}
      {data?.length === 0 ? <EmptyBlock>{t('complaints.admin.empty')}</EmptyBlock> : null}
      <div className="space-y-3">
        {data?.map((c) => (
          <article key={c.id} className="space-y-2 rounded-md border p-4">
            <div className="flex flex-wrap items-center gap-2">
              <Badge variant="secondary">{t(`complaints.category.${c.category}`)}</Badge>
              <Badge variant={c.status === 'open' ? 'default' : 'outline'}>{t(`complaints.admin.${c.status}`)}</Badge>
              <Badge variant={c.mail_status === 'failed' ? 'destructive' : 'outline'}>{t(`complaints.admin.mail.${c.mail_status}`)}</Badge>
              <time dateTime={c.created_at} className="text-muted-foreground ml-auto text-xs">
                {date.format(new Date(c.created_at))}
              </time>
            </div>
            <p className="break-words whitespace-pre-wrap">{c.description}</p>
            <div className="text-muted-foreground space-y-0.5 text-xs">
              <p>
                {t('complaints.admin.from', {
                  name: c.submitter_name ?? t('complaints.admin.noName'),
                  role: t(`complaints.admin.role.${c.submitter_role}`),
                })}
              </p>
              {c.classroom_name ? <p>{t('complaints.admin.classroom', { name: c.classroom_name })}</p> : null}
              {c.conversation_id ? <p>{t('complaints.admin.conversation', { id: c.conversation_id })}</p> : null}
            </div>
            <div className="flex flex-wrap gap-2">
              <Button
                size="sm"
                variant="outline"
                disabled={busy === c.id}
                onClick={() => void act(c.id, () => api.patch(`/api/admin/complaints/${encodeURIComponent(c.id)}`, { status: c.status === 'open' ? 'resolved' : 'open' }))}
              >
                {t(c.status === 'open' ? 'complaints.admin.resolve' : 'complaints.admin.reopen')}
              </Button>
              {/* 沒寄出的都能重寄：failed；卡在 pending（寄送途中程序重啟）；skipped（設定 SMTP 之前送出的） */}
              {c.mail_status !== 'sent' ? (
                <Button
                  size="sm"
                  variant="outline"
                  disabled={busy === c.id}
                  onClick={() => void act(c.id, () => api.post(`/api/admin/complaints/${encodeURIComponent(c.id)}/resend`))}
                >
                  {t('complaints.admin.resend')}
                </Button>
              ) : null}
            </div>
          </article>
        ))}
      </div>
    </Page>
  )
}
