import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Card, CardContent } from '@/components/ui/card'
import { ClaimGroups } from '@/features/dashboard/claim-groups'
import { Stat } from '@/features/dashboard/distribution'
import { useT } from '@/i18n'
import type { ClassDashboard } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

/** 教師：班上論點分布，以活動為單位（S-02.4：只給教師看，不設最小群體門檻；不含任何學生姓名或對話內容）。 */
export function ClassDashboardPage() {
  const t = useT()
  const { data, error, loading, reload } = useFetch<ClassDashboard>('/api/dashboard/class')

  return (
    <Page title={t('dashboard.class.title')}>
      <p className="text-muted-foreground text-sm">{t('dashboard.class.intro')}</p>
      {loading && !data ? <LoadingBlock /> : null}
      {error ? <ErrorBlock error={error} onRetry={reload} /> : null}
      {data ? <ClassBody d={data} onChanged={reload} /> : null}
    </Page>
  )
}

function ClassBody({ d, onChanged }: { d: ClassDashboard; onChanged: () => void }) {
  const t = useT()
  return (
    <div className="space-y-6">
      <div className="grid grid-cols-3 gap-2 sm:gap-3">
        <Stat label={t('dashboard.class.students')} value={d.students_total} />
        <Stat label={t('dashboard.class.participating')} value={d.students_participating} />
        <Stat label={t('dashboard.completed')} value={d.completed} />
      </div>
      {d.masked ? (
        <Alert role="status">
          <AlertDescription>{t('dashboard.class.masked')}</AlertDescription>
        </Alert>
      ) : d.completed === 0 ? (
        <EmptyBlock>{t('dashboard.class.empty')}</EmptyBlock>
      ) : (
        <div className="space-y-3">
          <h2 className="text-lg font-medium">{t('dashboard.class.byActivity')}</h2>
          <p className="text-muted-foreground text-sm">{t('dashboard.claims.help')}</p>
          {d.activities.map((activity) => (
            <Card key={activity.activity_id}>
              <CardContent className="space-y-4">
                <div className="flex flex-wrap items-baseline justify-between gap-2">
                  <h3 className="font-medium break-words">{activity.title}</h3>
                  <span className="text-muted-foreground text-sm">{t('dashboard.class.activityCompleted', { count: activity.completed })}</span>
                </div>
                <ClaimGroups activity={activity} onChanged={onChanged} canRegroup />
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  )
}
