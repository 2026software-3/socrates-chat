import { useParams } from 'react-router'
import { ErrorBlock, LoadingBlock, Page } from '@/components/page'
import { Badge } from '@/components/ui/badge'
import { useClassroomsBase } from '@/features/classrooms/use-base'
import { useI18n, useT } from '@/i18n'
import { formatDate } from '@/lib/format'
import type { Message, Transcript } from '@/lib/types'
import { cn } from '@/lib/utils'
import { useFetch } from '@/lib/use-fetch'

function Entry({ message, studentName }: { message: Message; studentName: string }) {
  const t = useT()
  const fromStudent = message.role === 'student'
  return (
    <li className={cn('flex flex-col gap-1', fromStudent ? 'items-end' : 'items-start')}>
      <div className="text-muted-foreground flex items-center gap-2 text-xs">
        <span className="font-medium">{fromStudent ? studentName : t('teacher.transcript.ai')}</span>
        {message.source !== 'text' ? <Badge variant="secondary">{t('teacher.transcript.voice')}</Badge> : null}
      </div>
      <p
        className={cn(
          'max-w-[85%] rounded-lg px-3 py-2 text-sm break-words whitespace-pre-wrap',
          fromStudent ? 'bg-primary text-primary-foreground' : 'bg-muted',
        )}
      >
        {message.content}
      </p>
    </li>
  )
}

/** 教師（或管理者，內容為 `message`）唯讀檢視一場已結束的活動對話（S-02.1）。沒有輸入框，也不能送出任何東西。 */
export function TranscriptPage() {
  const t = useT()
  const { lang } = useI18n()
  const base = useClassroomsBase()
  const { classroomId, conversationId } = useParams()
  const transcript = useFetch<Transcript>(`/api/summaries/${encodeURIComponent(conversationId ?? '')}/messages`)
  const data = transcript.data
  if (transcript.error) return <ErrorBlock error={transcript.error} onRetry={transcript.reload} />
  if (!data) return <LoadingBlock />

  const studentName = data.student_name ?? t('teacher.summaries.unnamed')
  return (
    <Page
      title={data.title}
      description={t('teacher.transcript.endedAt', { date: formatDate(data.ended_at, lang) })}
      crumbs={[
        { label: t('classrooms.nav'), to: base },
        { label: studentName, to: `${base}/${classroomId ?? data.classroom_id}/students/${data.student_id}` },
        { label: data.title },
      ]}
    >
      <p className="text-muted-foreground text-sm">{t('teacher.transcript.readOnly')}</p>
      <ol aria-label={t('teacher.transcript.list')} className="space-y-4">
        {data.messages.map((m) => (
          <Entry key={m.id} message={m} studentName={studentName} />
        ))}
      </ol>
    </Page>
  )
}
