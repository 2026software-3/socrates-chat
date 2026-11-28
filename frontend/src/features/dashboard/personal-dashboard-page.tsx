import { Link } from 'react-router'
import { Award, CircleCheck, FileText, ListChecks, Lock, MessageSquare } from 'lucide-react'
import { Alert } from '@/components/vedute/alert'
import { Button } from '@/components/vedute/button'
import { Skeleton, SkeletonRegion } from '@/components/vedute/skeleton'
import { StackBar } from '@/components/vedute/stack-bar'
import { formatDate } from '@/lib/format'
import { errorText, useI18n, useT } from '@/i18n'
import type { PersonalDashboard } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

/** 學生個人檔案（S-18、F-41.17）裡的「統計資料」頁籤內容：自己的討論統計與最近完成的討論。 */
export function PersonalDashboardContent() {
  const t = useT()
  const { data, error, loading, reload } = useFetch<PersonalDashboard>('/api/dashboard/me')

  if (loading && !data) {
    return (
      <SkeletonRegion loadingLabel={t('dashboard.me.loading')} className="space-y-6">
        <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
          <Skeleton className="h-24 w-full" />
          <Skeleton className="h-24 w-full" />
          <Skeleton className="h-24 w-full" />
        </div>
        <div className="space-y-2.5">
          <Skeleton className="h-6 w-40" />
          <Skeleton className="h-14 w-full" />
          <Skeleton className="h-14 w-full" />
        </div>
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
          <Skeleton className="h-28 w-full" />
          <Skeleton className="h-28 w-full" />
        </div>
      </SkeletonRegion>
    )
  }

  if (error) {
    return (
      <Alert kind="error" className="flex flex-wrap items-center justify-between gap-3">
        <span>{errorText(t, error)}</span>
        <Button variant="quiet" onClick={reload}>
          {t('common.retry')}
        </Button>
      </Alert>
    )
  }

  return data ? <Body d={data} /> : null
}

function Body({ d }: { d: PersonalDashboard }) {
  const t = useT()
  const { lang } = useI18n()
  const inProgress = Math.max(0, d.total_conversations - d.completed)

  const statCards = [
    {
      id: 'completed',
      icon: CircleCheck,
      label: t('dashboard.me.completedDiscussions'),
      value: d.completed,
      numeralClass: 'text-aerugo',
    },
    {
      id: 'in-progress',
      icon: MessageSquare,
      label: t('dashboard.me.inProgressDiscussions'),
      value: inProgress,
      numeralClass: 'text-ink',
    },
    {
      id: 'total',
      icon: ListChecks,
      label: t('dashboard.me.conversations'),
      value: d.total_conversations,
      numeralClass: 'text-ink',
    },
  ] as const

  return (
    <div className="space-y-6">
      {/* 第一排三張數字卡（桌機與平板三欄，手機橫向一列、數字靠右） */}
      <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
        {statCards.map((card) => {
          const Icon = card.icon
          return (
            <div
              key={card.id}
              className="flex items-center justify-between gap-3 rounded-sm border border-ink-faint bg-surface px-4 py-3.5 md:flex-col md:items-start md:justify-between md:p-4"
            >
              <div className="flex items-center gap-2 text-ink-soft">
                <Icon className="size-4 shrink-0" strokeWidth={1.5} aria-hidden="true" />
                <span className="type-note">{card.label}</span>
              </div>
              <p className={`font-serif text-4xl leading-none font-bold tabular-nums ${card.numeralClass}`}>
                {card.value}
              </p>
            </div>
          )
        })}
      </div>

      {/* 最近完成的討論 */}
      {d.completed === 0 ? (
        <div className="flex flex-col items-center gap-4 rounded-sm border border-ink-faint bg-paper p-6 text-center">
          <p className="type-body text-ink-soft">{t('dashboard.me.empty')}</p>
          <Button asChild>
            <Link to="/">{t('dashboard.me.start')}</Link>
          </Button>
        </div>
      ) : (
        <section className="space-y-3" aria-labelledby="me-recent">
          <h2 id="me-recent" className="type-h3 text-ink">
            {t('dashboard.me.recent')}
          </h2>
          <ul className="divide-y divide-ink-faint rounded-sm border border-ink-faint bg-surface">
            {d.recent.map((r) => (
              <li key={r.conversation_id} className="flex flex-wrap items-center justify-between gap-3 px-4 py-3">
                <div className="flex min-w-0 flex-1 flex-wrap items-baseline gap-x-3 gap-y-1">
                  <span className="type-body-strong min-w-0 break-words text-ink">{r.title}</span>
                  {r.ended_at ? (
                    <span className="type-note shrink-0 text-ink-soft">{formatDate(r.ended_at, lang)}</span>
                  ) : null}
                </div>
                <Button variant="quiet" asChild>
                  <Link to={`/conversations/${r.conversation_id}`}>
                    <FileText aria-hidden="true" />
                    {t('dashboard.me.viewSummary')}
                  </Link>
                </Button>
              </li>
            ))}
          </ul>
        </section>
      )}

      {/* 下排兩張：對話次數（龐貝藍長條）與點數與成就（赭金圖示） */}
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <section
          aria-labelledby="me-turns-heading"
          className="flex flex-col justify-between gap-3 rounded-sm border border-ink-faint bg-surface p-4"
        >
          <div className="flex items-baseline justify-between gap-2">
            <h2 id="me-turns-heading" className="type-h3 text-ink">
              {t('dashboard.me.turnChartTitle')}
            </h2>
            <span className="type-note text-ink-soft tabular-nums">
              {t('dashboard.me.turnsOf', { count: d.total_turns })}
            </span>
          </div>
          <StackBar
            size="md"
            showLegend={false}
            ariaLabel={t('dashboard.me.turnChartTitle')}
            segments={[{ label: t('dashboard.me.turns'), value: d.total_turns, tone: 'caeruleum' }]}
          />
          <p className="type-note text-ink-soft">{t('dashboard.me.turnChartPending', { count: d.total_turns })}</p>
        </section>

        <section
          aria-labelledby="me-points-heading"
          className="flex flex-col justify-between gap-3 rounded-sm border border-ink-faint bg-surface p-4"
        >
          <div className="flex items-center gap-2">
            <Award className="size-4.5 shrink-0 text-ochra" strokeWidth={1.5} aria-hidden="true" />
            <h2 id="me-points-heading" className="type-h3 text-ink">
              {t('dashboard.me.pointsTitle')}
            </h2>
          </div>
          <p className="type-note text-ink-soft">{t('dashboard.me.pointsPending')}</p>
        </section>
      </div>

      {/* 最下方隱私提示 */}
      <p className="type-note flex items-center gap-1.5 text-ink-soft">
        <Lock className="size-3.5 shrink-0" strokeWidth={1.5} aria-hidden="true" />
        <span>{t('dashboard.me.privacyNote')}</span>
      </p>
    </div>
  )
}