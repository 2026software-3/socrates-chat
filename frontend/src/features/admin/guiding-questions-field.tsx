import { ArrowDown, ArrowUp, Plus, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { useT } from '@/i18n'

/** 題目的引導問題上限：最多 5 個、每個最多 200 字（S-04.8）；後端同樣限制。 */
export const MAX_GUIDING_QUESTIONS = 5
export const MAX_GUIDING_QUESTION_LENGTH = 200

/** 題目表單的引導問題清單：新增、移除、上下排序，並顯示數量與字數。空白項目由呼叫端在送出時略過。 */
export function GuidingQuestionsField({
  idPrefix,
  value,
  onChange,
}: {
  idPrefix: string
  value: string[]
  onChange: (value: string[]) => void
}) {
  const t = useT()
  const full = value.length >= MAX_GUIDING_QUESTIONS

  function move(i: number, delta: number) {
    const next = [...value]
    ;[next[i], next[i + delta]] = [next[i + delta], next[i]]
    onChange(next)
  }

  return (
    <fieldset className="space-y-2">
      <legend className="flex w-full items-center justify-between text-sm font-medium">
        <span>{t('admin.topics.guiding.legend')}</span>
        <span className="text-muted-foreground font-normal">{`${value.length} / ${MAX_GUIDING_QUESTIONS}`}</span>
      </legend>
      <p className="text-muted-foreground text-xs">{t('admin.topics.guiding.hint')}</p>
      {value.map((q, i) => (
        <div key={i} className="space-y-1">
          <Label htmlFor={`${idPrefix}-gq-${i}`} className="sr-only">
            {t('admin.topics.guiding.item', { n: i + 1 })}
          </Label>
          <div className="flex items-start gap-1">
            <Textarea
              id={`${idPrefix}-gq-${i}`}
              rows={2}
              maxLength={MAX_GUIDING_QUESTION_LENGTH}
              value={q}
              onChange={(e) => onChange(value.map((x, j) => (j === i ? e.target.value : x)))}
            />
            <Button type="button" variant="ghost" size="icon" className="size-8" disabled={i === 0} aria-label={t('admin.topics.guiding.up', { n: i + 1 })} onClick={() => move(i, -1)}>
              <ArrowUp aria-hidden />
            </Button>
            <Button type="button" variant="ghost" size="icon" className="size-8" disabled={i === value.length - 1} aria-label={t('admin.topics.guiding.down', { n: i + 1 })} onClick={() => move(i, 1)}>
              <ArrowDown aria-hidden />
            </Button>
            <Button type="button" variant="ghost" size="icon" className="size-8" aria-label={t('admin.topics.guiding.remove', { n: i + 1 })} onClick={() => onChange(value.filter((_, j) => j !== i))}>
              <X aria-hidden />
            </Button>
          </div>
          <p className="text-muted-foreground text-right text-xs">{`${[...q].length} / ${MAX_GUIDING_QUESTION_LENGTH}`}</p>
        </div>
      ))}
      <Button type="button" variant="outline" disabled={full} onClick={() => onChange([...value, ''])}>
        <Plus aria-hidden />
        {t('admin.topics.guiding.add')}
      </Button>
      {full ? <p className="text-muted-foreground text-xs">{t('admin.topics.guiding.full')}</p> : null}
    </fieldset>
  )
}
