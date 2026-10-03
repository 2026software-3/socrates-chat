import { cn } from '@/lib/utils'
import { useT } from '@/i18n'

const STAGES = [1, 2, 3] as const

/** 追問階段（1 釐清 / 2 論證 / 3 挑戰）的小型進度指示。 */
export function StageIndicator({ stage }: { stage: 1 | 2 | 3 }) {
  const t = useT()
  return (
    <div className="space-y-1">
      <ol aria-label={t('chat.stage.label')} className="flex flex-wrap items-center gap-2" data-testid="stage-indicator">
        {STAGES.map((s) => (
          <li
            key={s}
            aria-current={s === stage ? 'step' : undefined}
            className={cn(
              'flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs',
              s === stage ? 'bg-primary text-primary-foreground border-primary' : 'text-muted-foreground',
              s < stage && 'border-primary/40',
            )}
          >
            <span aria-hidden>{s}</span>
            <span>{t(`chat.stage.${s}`)}</span>
          </li>
        ))}
      </ol>
    </div>
  )
}
