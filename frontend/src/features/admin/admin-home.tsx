import { Link } from 'react-router'
import { ArrowRight, BookMarked, GraduationCap, Users, type LucideIcon } from 'lucide-react'
import { PageHeader } from '@/components/page'
import { Alert } from '@/components/vedute/alert'
import { Button } from '@/components/vedute/button'
import { AdminCard } from '@/features/admin/admin-ui'
import { SystemOverview } from '@/features/admin/system-overview'
import { useT, type MessageKey } from '@/i18n'
import type { ClassDashboard, EmailList, Topic } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

type Props = {
  icon: LucideIcon
  numeral: string
  to: string
  title: MessageKey
  desc: MessageKey
  action: MessageKey
  count: string
}

function OpsCard({ icon: Icon, numeral, to, title, desc, action, count }: Props) {
  const t = useT()
  return (
    <AdminCard className="flex flex-col justify-between p-5">
      <div className="space-y-3">
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-2.5">
            <span className="inline-flex size-9 shrink-0 items-center justify-center border border-ink-faint bg-paper text-ink">
              <Icon className="size-4" strokeWidth={1.5} aria-hidden="true" />
            </span>
            <span className="type-body-strong text-ink">{t(title)}</span>
          </div>
          <span aria-hidden="true" className="type-latin-label text-ink-soft">
            {numeral}
          </span>
        </div>
        <p className="type-h1 tabular-nums text-ink">{count}</p>
        <p className="type-note text-ink-soft">{t(desc)}</p>
      </div>
      <div className="mt-4 border-t border-ink-faint pt-3">
        <Button asChild variant="quiet" className="w-full justify-between">
          <Link to={to}>
            <span>{t(action)}</span>
            <ArrowRight className="size-4" strokeWidth={1.5} aria-hidden="true" />
          </Link>
        </Button>
      </div>
    </AdminCard>
  )
}

/** 系統管理者首頁：只維運，不參與討論。包含服務健康度、系統概況與維運入口（S-09.3）。 */
export function AdminHome() {
  const t = useT()
  const teachers = useFetch<EmailList>('/api/admin/teachers')
  const students = useFetch<ClassDashboard>('/api/dashboard/class')
  const topics = useFetch<Topic[]>('/api/topics')
  const n = (v: number | undefined) => (v === undefined ? '–' : String(v))

  return (
    <div className="space-y-8">
      <PageHeader title={t('admin.home.title')} description={t('admin.home.tagline')} />
      <SystemOverview />
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <OpsCard
          icon={Users}
          numeral="II"
          to="/admin/roster"
          title="admin.home.roster"
          desc="admin.home.rosterDesc"
          action="admin.home.rosterAction"
          count={n(students.data?.students_total)}
        />
        <OpsCard
          icon={GraduationCap}
          numeral="III"
          to="/admin/teachers"
          title="admin.home.teachers"
          desc="admin.home.teachersDesc"
          action="admin.home.teachersAction"
          count={n(teachers.data?.emails.length)}
        />
        <OpsCard
          icon={BookMarked}
          numeral="V"
          to="/admin/topics"
          title="admin.home.topics"
          desc="admin.home.topicsDesc"
          action="admin.home.topicsAction"
          count={n(topics.data?.filter((x) => x.is_active).length)}
        />
      </div>
      <Alert kind="info">{t('admin.home.note')}</Alert>
    </div>
  )
}
