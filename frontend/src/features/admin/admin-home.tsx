import { Link } from 'react-router'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardDescription, CardFooter, CardHeader, CardTitle } from '@/components/ui/card'
import { useT, type MessageKey } from '@/i18n'
import type { EmailList, Topic } from '@/lib/types'
import { useFetch } from '@/lib/use-fetch'

type Props = { to: string; title: MessageKey; desc: MessageKey; action: MessageKey; count: string }

function OpsCard({ to, title, desc, action, count }: Props) {
  const t = useT()
  return (
    <Card className="justify-between">
      <CardHeader>
        <CardDescription>{t(title)}</CardDescription>
        <CardTitle className="text-3xl tabular-nums">{count}</CardTitle>
        <CardDescription>{t(desc)}</CardDescription>
      </CardHeader>
      <CardFooter>
        <Button asChild variant="outline">
          <Link to={to}>{t(action)}</Link>
        </Button>
      </CardFooter>
    </Card>
  )
}

/** 系統管理者首頁：只維運，不參與討論。三項維運工作與目前數量。 */
export function AdminHome() {
  const t = useT()
  const teachers = useFetch<EmailList>('/api/admin/teachers')
  const roster = useFetch<EmailList>('/api/roster')
  const topics = useFetch<Topic[]>('/api/admin/topics')
  const n = (v: number | undefined) => (v === undefined ? '–' : String(v))

  return (
    <div className="space-y-6">
      <div className="from-primary/10 to-background space-y-1 rounded-xl border bg-gradient-to-br p-6">
        <h1 className="text-2xl font-semibold tracking-tight sm:text-3xl">{t('admin.home.title')}</h1>
        <p className="text-muted-foreground max-w-prose text-sm">{t('admin.home.tagline')}</p>
      </div>
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <OpsCard
          to="/admin/teachers"
          title="admin.home.teachers"
          desc="admin.home.teachersDesc"
          action="admin.home.teachersAction"
          count={n(teachers.data?.emails.length)}
        />
        <OpsCard
          to="/admin/roster"
          title="admin.home.roster"
          desc="admin.home.rosterDesc"
          action="admin.home.rosterAction"
          count={n(roster.data?.emails.length)}
        />
        <OpsCard
          to="/admin/topics"
          title="admin.home.topics"
          desc="admin.home.topicsDesc"
          action="admin.home.topicsAction"
          count={n(topics.data?.filter((x) => x.is_active).length)}
        />
      </div>
      <Alert>
        <AlertDescription>{t('admin.home.note')}</AlertDescription>
      </Alert>
    </div>
  )
}
