import { Fragment } from 'react'
import { cn } from '@/lib/utils'
import { useT } from '@/i18n'

const STAGES = [1, 2, 3] as const

/** 追問階段（1 釐清 / 2 論證 / 3 挑戰）的小型進度指示：目前階段實心，其餘外框。 */
export function StageIndicator({ stage }: { stage: 1 | 2 | 3 }) {
  const t = useT()
  return (
    <ol aria-label={t('chat.stage.label')} className="flex flex-wrap items-center gap-2" data-testid="stage-indicator">
      {STAGES.map((s, i) => (
        <Fragment key={s}>
          {i > 0 ? (
            <li aria-hidden className="text-muted-foreground list-none">
              →
            </li>
          ) : null}
          <li
            aria-current={s === stage ? 'step' : undefined}
            className={cn(
              'rounded-full border px-2.5 py-1 text-xs',
              s === stage ? 'border-foreground bg-foreground text-background' : 'text-muted-foreground',
            )}
          >
            {t(`chat.stage.${s}`)}
          </li>
        </Fragment>
      ))}
    </ol>
  )
}
