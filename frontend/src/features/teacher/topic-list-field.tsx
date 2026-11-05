import { useState } from 'react'
import { ArrowDown, ArrowUp, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { useT } from '@/i18n'
import type { Topic } from '@/lib/types'

/** 一個活動最多可含的題目數（S-04.7）；後端同樣限制，超過回 `invalid_topic`。 */
const MAX_ACTIVITY_TOPICS = 5

const selectClass =
  'border-input bg-background h-9 w-full rounded-md border px-3 text-sm focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 outline-none disabled:opacity-50'

/**
 * 活動的題目清單：依序列出已選題目，可上下移動與移除，並從題庫加入（最多 5 個）。
 * `lockedCount` 為已有對話而鎖定的題目數：它們固定排在最前面，不能移除、更換或移動，其後的新題目仍可自由調整。
 */
export function TopicListField({
  idPrefix,
  topicIds,
  topics,
  lockedCount,
  invalid,
  failed,
  onChange,
}: {
  idPrefix: string
  topicIds: string[]
  /** 可加入的題目（啟用中的題庫題目）。 */
  topics: Topic[]
  lockedCount: number
  /** 送出時清單是空的。 */
  invalid: boolean
  /** 題庫載入失敗。 */
  failed: boolean
  onChange: (topicIds: string[]) => void
}) {
  const t = useT()
  const [pick, setPick] = useState('')
  const titleOf = (id: string) => topics.find((tp) => tp.id === id)?.title ?? t('teacher.activities.topicUnavailable')
  const full = topicIds.length >= MAX_ACTIVITY_TOPICS
  const addable = topics.filter((tp) => !topicIds.includes(tp.id))
  const errorId = `${idPrefix}-topic-error`
  const hintId = `${idPrefix}-topic-hint`

  function move(from: number, to: number) {
    const next = [...topicIds]
    ;[next[from], next[to]] = [next[to]!, next[from]!]
    onChange(next)
  }

  function add() {
    if (!pick || full || topicIds.includes(pick)) return
    onChange([...topicIds, pick])
    setPick('')
  }

  return (
    <fieldset className="space-y-2" aria-describedby={hintId}>
      <legend className="text-sm leading-none font-medium">{t('teacher.activities.topicLabel')}</legend>
      <p id={hintId} className="text-muted-foreground text-sm">
        {t(lockedCount > 0 ? 'teacher.activities.topicLockedHint' : 'teacher.activities.topicHint')}
      </p>
      {topicIds.length > 0 ? (
        <ol className="list-decimal space-y-1.5 pl-5">
          {topicIds.map((id, index) => {
            const title = titleOf(id)
            const locked = index < lockedCount
            // 已鎖定的題目固定在最前面：第一個新題目不能往上越過它們
            const canUp = !locked && index > lockedCount
            const canDown = !locked && index < topicIds.length - 1
            return (
              <li key={id} className="pl-1">
                <div className="flex items-center gap-1">
                  <span className="min-w-0 flex-1 text-sm break-words">{title}</span>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="size-8"
                    disabled={!canUp}
                    aria-label={t('teacher.activities.topicMoveUp', { title })}
                    onClick={() => move(index, index - 1)}
                  >
                    <ArrowUp aria-hidden />
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="size-8"
                    disabled={!canDown}
                    aria-label={t('teacher.activities.topicMoveDown', { title })}
                    onClick={() => move(index, index + 1)}
                  >
                    <ArrowDown aria-hidden />
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon"
                    className="size-8"
                    disabled={locked}
                    aria-label={t('teacher.activities.topicRemove', { title })}
                    onClick={() => onChange(topicIds.filter((x) => x !== id))}
                  >
                    <X aria-hidden />
                  </Button>
                </div>
              </li>
            )
          })}
        </ol>
      ) : (
        <p className="text-muted-foreground text-sm">{t('teacher.activities.topicEmpty')}</p>
      )}
      {full ? <p className="text-muted-foreground text-sm">{t('teacher.activities.topicLimit')}</p> : null}
      <div className="space-y-1.5">
        <Label htmlFor={`${idPrefix}-topic-add`}>{t('teacher.activities.topicAddLabel')}</Label>
        <div className="flex items-center gap-2">
          <select
            id={`${idPrefix}-topic-add`}
            className={selectClass}
            value={pick}
            disabled={full}
            aria-invalid={invalid}
            aria-describedby={invalid ? errorId : undefined}
            onChange={(e) => setPick(e.target.value)}
          >
            <option value="" disabled>
              {t('teacher.activities.topicPlaceholder')}
            </option>
            {addable.map((tp) => (
              <option key={tp.id} value={tp.id}>
                {tp.title}
              </option>
            ))}
          </select>
          <Button type="button" variant="outline" disabled={full || !pick} onClick={add}>
            {t('teacher.activities.topicAddButton')}
          </Button>
        </div>
      </div>
      {invalid ? (
        <p id={errorId} role="alert" className="text-destructive text-sm">
          {t('teacher.activities.topicRequired')}
        </p>
      ) : null}
      {failed ? <p className="text-muted-foreground text-sm">{t('teacher.activities.topicsFailed')}</p> : null}
    </fieldset>
  )
}
