import { useT } from '@/i18n'
import { useFetch } from '@/lib/use-fetch'

/** 學生在教室資訊處看到「本教室的 AI 由 {供應商} 提供」（F-40.4）；尚未設定或過渡中不顯示，看不到模型與 key。 */
export function ClassroomProviderNote({ classroomId }: { classroomId: string }) {
  const t = useT()
  const { data } = useFetch<{ provider: { id: string; name: string } | null }>(`/api/classrooms/${encodeURIComponent(classroomId)}/ai-provider`)
  if (!data?.provider) return null
  return <p className="text-muted-foreground text-xs">{t('classroomAi.provider', { provider: data.provider.name })}</p>
}
