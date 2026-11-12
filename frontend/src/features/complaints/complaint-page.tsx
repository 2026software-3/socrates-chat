import { useState, type FormEvent } from 'react'
import { useSearchParams } from 'react-router'
import { Page } from '@/components/page'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { errorText, useT } from '@/i18n'
import { api } from '@/lib/api'

export const CATEGORIES = ['connection', 'voice', 'content', 'other'] as const
export type Category = (typeof CATEGORIES)[number]
/** 與後端 `complaints.description` 的上限一致（字元數）。 */
export const MAX_DESCRIPTION = 2000

/**
 * 回報問題（S-17）：學生與教師送出，內容以電子郵件寄給管理者；送出後不能追蹤也不會有回覆。
 * 從對話頁進入時網址帶 `?conversation=<id>`，只附上編號，不附對話內容。
 */
export function ComplaintPage() {
  const t = useT()
  const [params] = useSearchParams()
  const conversationId = params.get('conversation') || undefined
  const [category, setCategory] = useState<Category>('other')
  const [description, setDescription] = useState('')
  const [busy, setBusy] = useState(false)
  const [sent, setSent] = useState(false)
  const [error, setError] = useState<string>()

  async function submit(event: FormEvent) {
    event.preventDefault()
    if (busy) return
    const text = description.trim()
    setSent(false)
    if (!text) {
      setError(t('error.invalid_description'))
      return
    }
    setBusy(true)
    setError(undefined)
    try {
      await api.post('/api/complaints', {
        category,
        description: text,
        ...(conversationId ? { conversation_id: conversationId } : {}),
      })
      setSent(true)
      setDescription('')
    } catch (err) {
      setError(errorText(t, err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Page title={t('complaints.title')}>
      <form onSubmit={(event) => void submit(event)} className="max-w-2xl space-y-5">
        <p className="text-muted-foreground text-sm">{t('complaints.notice')}</p>
        <fieldset className="space-y-2">
          <legend className="text-sm font-medium">{t('complaints.category')}</legend>
          <div className="flex flex-wrap gap-x-5 gap-y-2">
            {CATEGORIES.map((c) => (
              <label key={c} className="flex items-center gap-2 text-sm">
                <input type="radio" name="category" value={c} checked={category === c} onChange={() => setCategory(c)} />
                {t(`complaints.category.${c}`)}
              </label>
            ))}
          </div>
        </fieldset>
        <div className="space-y-2">
          <Label htmlFor="complaint-description">{t('complaints.description')}</Label>
          <Textarea
            id="complaint-description"
            value={description}
            maxLength={MAX_DESCRIPTION}
            rows={6}
            onChange={(e) => setDescription(e.target.value)}
          />
          <p className="text-muted-foreground text-right text-xs">
            {t('complaints.counter', { count: [...description].length, max: MAX_DESCRIPTION })}
          </p>
        </div>
        {conversationId ? <p className="text-muted-foreground text-sm">{t('complaints.attached')}</p> : null}
        {error ? (
          <Alert variant="destructive">
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        ) : null}
        {sent ? <p role="status">{t('complaints.sent')}</p> : null}
        <Button type="submit" disabled={busy}>
          {busy ? t('complaints.submitting') : t('complaints.submit')}
        </Button>
      </form>
    </Page>
  )
}
