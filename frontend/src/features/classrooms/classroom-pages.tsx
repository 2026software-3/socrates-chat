import { Link, useParams } from 'react-router'
import { EmptyBlock, ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { LoadMore } from '@/components/load-more'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { useClassroomsBase } from '@/features/classrooms/use-base'
import { ClaimGroups } from '@/features/dashboard/claim-groups'
import { Stat } from '@/features/dashboard/distribution'
import { ActivitiesPanel } from '@/features/teacher/activities-page'
import { ClassroomRoster } from '@/features/teacher/roster-page'
import { SummaryCard } from '@/features/teacher/summaries-page'
import { useT } from '@/i18n'
import type { Activity, Classroom, ClassDashboard, Page as PageResult, TeacherSummary } from '@/lib/types'
import { useFetch, usePagedFetch } from '@/lib/use-fetch'

/** 依網址的教室 id 找教室；載入中、失敗與找不到都在這裡處理，找到才渲染 `children`。 */
function WithClassroom({ children }: { children: (classroom: Classroom) => React.ReactNode }) {
  const t = useT()
  const base = useClassroomsBase()
  const { classroomId } = useParams()
  const classrooms = useFetch<Classroom[]>('/api/classrooms')
  const classroom = classrooms.data?.find((c) => c.id === classroomId)
  if (classrooms.error) return <ErrorBlock error={classrooms.error} onRetry={classrooms.reload} />
  if (!classrooms.data) return <LoadingBlock />
  if (!classroom) {
    return (
      <Page title={t('error.not_found')}>
        <Link to={base} className="underline">
          {t('classrooms.backToList')}
        </Link>
      </Page>
    )
  }
  return <>{children(classroom)}</>
}

/** 單一教室：人數摘要、前往學生名單，以及該教室的討論活動（建立、管理，點活動看論點分布）。 */
export function ClassroomPage() {
  return <WithClassroom>{(classroom) => <ClassroomBody classroom={classroom} />}</WithClassroom>
}

function ClassroomBody({ classroom }: { classroom: Classroom }) {
  const t = useT()
  const base = useClassroomsBase()
  const stats = useFetch<ClassDashboard>(`/api/dashboard/class?classroom_id=${classroom.id}`)
  return (
    <Page
      title={classroom.name}
      crumbs={[{ label: t('classrooms.nav'), to: base }, { label: classroom.name }]}
      description={[classroom.year_label, classroom.description].filter(Boolean).join(' · ') || undefined}
      actions={
        <Button asChild variant="outline">
          <Link to={`${base}/${classroom.id}/students`}>{t('classrooms.students')}</Link>
        </Button>
      }
    >
      {classroom.is_archived ? (
        <Alert role="status">
          <AlertDescription>{t('classrooms.archived')}</AlertDescription>
        </Alert>
      ) : null}
      {stats.data ? (
        <div className="grid grid-cols-3 gap-2 sm:gap-3">
          <Stat label={t('dashboard.class.students')} value={stats.data.students_total} />
          <Stat label={t('dashboard.class.participating')} value={stats.data.students_participating} />
          <Stat label={t('dashboard.completed')} value={stats.data.completed} />
        </div>
      ) : null}
      <h2 className="text-xl font-medium">{t('teacher.activities.listHeading')}</h2>
      <ActivitiesPanel classroomId={classroom.id} detailBase={`${base}/${classroom.id}/activities`} />
    </Page>
  )
}

/** 教室的學生名單與帳號管理；每位學生可點進去看他的討論情形。 */
export function ClassroomStudentsPage() {
  const t = useT()
  const base = useClassroomsBase()
  return (
    <WithClassroom>
      {(classroom) => (
        <Page
          title={t('classrooms.students')}
          crumbs={[
            { label: t('classrooms.nav'), to: base },
            { label: classroom.name, to: `${base}/${classroom.id}` },
            { label: t('classrooms.students') },
          ]}
        >
          <ClassroomRoster classroom={classroom} />
        </Page>
      )}
    </WithClassroom>
  )
}

/** 單一活動：論點分布，以及每位學生的總結（可點進學生）。 */
export function ClassroomActivityPage() {
  const { activityId } = useParams()
  return <WithClassroom>{(classroom) => <ActivityBody classroom={classroom} activityId={activityId ?? ''} />}</WithClassroom>
}

function ActivityBody({ classroom, activityId }: { classroom: Classroom; activityId: string }) {
  const t = useT()
  const base = useClassroomsBase()
  const room = encodeURIComponent(classroom.id)
  // 活動可能已封存，所以兩種都查；一個教室的活動數不多，一次取 200 筆
  const active = useFetch<PageResult<Activity>>(`/api/activities?classroom_id=${room}&limit=200`)
  const archived = useFetch<PageResult<Activity>>(`/api/activities?classroom_id=${room}&limit=200&archived=true`)
  const dashboard = useFetch<ClassDashboard>(`/api/dashboard/class?classroom_id=${room}`)
  const summaries = usePagedFetch<TeacherSummary>(
    `/api/summaries?classroom_id=${room}&activity=${encodeURIComponent(activityId)}`,
  )
  const activity = [...(active.data?.items ?? []), ...(archived.data?.items ?? [])].find((a) => a.id === activityId)
  const distribution = dashboard.data?.activities.find((a) => a.activity_id === activityId)
  const loadingMeta = (active.loading || archived.loading) && !activity
  const title = activity?.title ?? t('classrooms.activity')

  return (
    <Page
      title={title}
      crumbs={[
        { label: t('classrooms.nav'), to: base },
        { label: classroom.name, to: `${base}/${classroom.id}` },
        { label: title },
      ]}
      description={activity?.description || undefined}
    >
      {loadingMeta ? <LoadingBlock /> : null}
      {!loadingMeta && !activity && !active.error && !archived.error ? <EmptyBlock>{t('error.not_found')}</EmptyBlock> : null}
      {active.error ? <ErrorBlock error={active.error} onRetry={active.reload} /> : null}

      <section className="space-y-3">
        <h2 className="text-lg font-medium">{t('classrooms.distribution')}</h2>
        {dashboard.error ? <ErrorBlock error={dashboard.error} onRetry={dashboard.reload} /> : null}
        {dashboard.loading && !dashboard.data ? <LoadingBlock /> : null}
        {dashboard.data?.masked ? (
          <Alert role="status">
            <AlertDescription>{t('dashboard.class.masked')}</AlertDescription>
          </Alert>
        ) : dashboard.data ? (
          distribution ? (
            <Card>
              <CardContent className="space-y-4">
                <p className="text-muted-foreground text-sm">
                  {t('dashboard.class.activityCompleted', { count: distribution.completed })}
                </p>
                <ClaimGroups activity={distribution} onChanged={dashboard.reload} canRegroup />
              </CardContent>
            </Card>
          ) : (
            <EmptyBlock>{t('dashboard.class.empty')}</EmptyBlock>
          )
        ) : null}
      </section>

      <section className="space-y-3">
        <h2 className="text-lg font-medium">{t('classrooms.studentSummaries')}</h2>
        <Alert>
          <AlertDescription>{t('teacher.summaries.privacyNote')}</AlertDescription>
        </Alert>
        {summaries.loading && !summaries.items ? <LoadingBlock /> : null}
        {summaries.error ? <ErrorBlock error={summaries.error} onRetry={summaries.reload} /> : null}
        {summaries.items && summaries.items.length === 0 ? <EmptyBlock>{t('teacher.summaries.empty')}</EmptyBlock> : null}
        {summaries.items && summaries.items.length > 0 ? (
          <ul className="space-y-3">
            {summaries.items.map((r) => (
              <SummaryCard key={r.conversation_id} row={r} />
            ))}
          </ul>
        ) : null}
        <LoadMore hasMore={summaries.hasMore} loading={summaries.loadingMore} error={summaries.moreError} onLoadMore={summaries.loadMore} />
      </section>
    </Page>
  )
}

/** 單一學生在這間教室的討論情形：他完成的每場對話總結（不含原文）。 */
export function ClassroomStudentPage() {
  const { studentId } = useParams()
  return <WithClassroom>{(classroom) => <StudentBody classroom={classroom} studentId={studentId ?? ''} />}</WithClassroom>
}

function StudentBody({ classroom, studentId }: { classroom: Classroom; studentId: string }) {
  const t = useT()
  const base = useClassroomsBase()
  const summaries = usePagedFetch<TeacherSummary>(
    `/api/summaries?classroom_id=${encodeURIComponent(classroom.id)}&student=${encodeURIComponent(studentId)}`,
  )
  const first = summaries.items?.[0]
  const name = first ? (first.student_name ?? first.student_email) : t('classrooms.student')
  return (
    <Page
      title={name}
      description={first?.student_name ? first.student_email : undefined}
      crumbs={[
        { label: t('classrooms.nav'), to: base },
        { label: classroom.name, to: `${base}/${classroom.id}` },
        { label: t('classrooms.students'), to: `${base}/${classroom.id}/students` },
        { label: name },
      ]}
    >
      <Alert>
        <AlertDescription>{t('teacher.summaries.privacyNote')}</AlertDescription>
      </Alert>
      {summaries.loading && !summaries.items ? <LoadingBlock /> : null}
      {summaries.error ? <ErrorBlock error={summaries.error} onRetry={summaries.reload} /> : null}
      {summaries.items && summaries.items.length === 0 ? <EmptyBlock>{t('classrooms.studentEmpty')}</EmptyBlock> : null}
      {summaries.items && summaries.items.length > 0 ? (
        <ul className="space-y-3">
          {summaries.items.map((r) => (
            <SummaryCard key={r.conversation_id} row={r} linkStudent={false} />
          ))}
        </ul>
      ) : null}
      <LoadMore hasMore={summaries.hasMore} loading={summaries.loadingMore} error={summaries.moreError} onLoadMore={summaries.loadMore} />
    </Page>
  )
}
