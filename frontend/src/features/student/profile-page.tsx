import { useState, type FormEvent } from 'react'
import { useAuth, useMe } from '@/auth/auth-context'
import { Page } from '@/components/page'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { errorText, useT } from '@/i18n'
import { api } from '@/lib/api'
import type { Me } from '@/lib/types'

/** 顯示名稱的長度上限；後端才是準則（S-08.2），這裡只是方便使用者。 */
const DISPLAY_NAME_MAX = 20

/** 學生自訂顯示名稱：留空會清除，介面回到顯示電子郵件。 */
export function ProfilePage() {
  const t = useT()
  const me = useMe()
  const { refresh } = useAuth()
  const [name, setName] = useState(me.display_name ?? '')
  const [saving, setSaving] = useState(false)
  const [saved, setSaved] = useState(false)
  const [error, setError] = useState<unknown>(null)

  async function submit(e: FormEvent) {
    e.preventDefault()
    setSaving(true)
    setSaved(false)
    setError(null)
    try {
      const updated = await api.patch<Me>('/api/me', { display_name: name.trim() || null })
      setName(updated.display_name ?? '')
      setSaved(true)
      await refresh()
    } catch (err) {
      setError(err)
    } finally {
      setSaving(false)
    }
  }

  return (
    <Page title={t('profile.title')}>
      <form onSubmit={(e) => void submit(e)} className="max-w-sm space-y-3">
        {error ? (
          <Alert variant="destructive" role="alert">
            <AlertTitle>{errorText(t, error)}</AlertTitle>
          </Alert>
        ) : null}
        <div className="space-y-1.5">
          <Label htmlFor="profile-name">{t('profile.name')}</Label>
          <Input
            id="profile-name"
            maxLength={DISPLAY_NAME_MAX}
            autoComplete="nickname"
            value={name}
            onChange={(e) => {
              setName(e.target.value)
              setSaved(false)
            }}
          />
          <p className="text-muted-foreground text-xs">{t('profile.hint', { max: DISPLAY_NAME_MAX })}</p>
        </div>
        <div className="flex items-center gap-3">
          <Button type="submit" disabled={saving}>
            {t('profile.save')}
          </Button>
          {saved ? (
            <p role="status" className="text-sm">
              {t('profile.saved')}
            </p>
          ) : null}
        </div>
      </form>
    </Page>
  )
}
