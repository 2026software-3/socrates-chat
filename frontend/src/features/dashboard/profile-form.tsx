import { useState, type FormEvent } from 'react'
import { useAuth } from '@/auth/auth-context'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { errorText, useT } from '@/i18n'
import { api } from '@/lib/api'
import type { Me } from '@/lib/types'

/** 顯示名稱的長度上限；後端才是準則（S-08.2），這裡只是方便使用者。 */
const DISPLAY_NAME_MAX = 30

/** 與後端一樣以 Unicode 字元計（emoji 算 1，不像 maxLength 以 UTF-16 計），去頭尾空白後再算。 */
const nameLength = (value: string) => Array.from(value.trim()).length

/** 個人儀表板上的顯示名稱設定：留空會清除，介面回到顯示電子郵件。 */
export function ProfileForm() {
  const auth = useAuth()
  // 登入者載入完成前不顯示（外框保證實際使用時一定已登入）
  return auth.status === 'authed' ? <Form me={auth.me} update={auth.update} /> : null
}

function Form({ me, update }: { me: Me; update: (me: Me) => void }) {
  const t = useT()
  const [name, setName] = useState(me.display_name ?? '')
  const [saving, setSaving] = useState(false)
  const [saved, setSaved] = useState(false)
  const [error, setError] = useState<unknown>(null)
  const length = nameLength(name)
  const tooLong = length > DISPLAY_NAME_MAX

  async function submit(e: FormEvent) {
    e.preventDefault()
    setSaving(true)
    setSaved(false)
    setError(null)
    try {
      const updated = await api.patch<Me>('/api/me', { display_name: name.trim() || null })
      setName(updated.display_name ?? '')
      setSaved(true)
      update(updated)
    } catch (err) {
      setError(err)
    } finally {
      setSaving(false)
    }
  }

  return (
    <Card>
      <CardContent>
        <form onSubmit={(e) => void submit(e)} aria-labelledby="profile-title" className="max-w-sm space-y-3">
          <h2 id="profile-title" className="text-lg font-medium">
            {t('profile.title')}
          </h2>
          {error ? (
            <Alert variant="destructive" role="alert">
              <AlertTitle>{errorText(t, error)}</AlertTitle>
            </Alert>
          ) : null}
          <div className="space-y-1.5">
            <Label htmlFor="profile-name">{t('profile.name')}</Label>
            <Input
              id="profile-name"
              autoComplete="nickname"
              aria-invalid={tooLong}
              aria-describedby="profile-name-hint"
              value={name}
              onChange={(e) => {
                setName(e.target.value)
                setSaved(false)
              }}
            />
            <p id="profile-name-hint" className="text-muted-foreground flex justify-between gap-2 text-xs">
              <span>{t('profile.hint', { max: DISPLAY_NAME_MAX })}</span>
              <span className={tooLong ? 'text-destructive font-medium' : undefined}>
                {length} / {DISPLAY_NAME_MAX}
              </span>
            </p>
            {tooLong ? <p className="text-destructive text-xs">{t('profile.tooLong', { max: DISPLAY_NAME_MAX })}</p> : null}
          </div>
          <div className="flex items-center gap-3">
            <Button type="submit" disabled={saving || tooLong}>
              {t('profile.save')}
            </Button>
            {saved ? (
              <p role="status" className="text-sm">
                {t('profile.saved')}
              </p>
            ) : null}
          </div>
        </form>
      </CardContent>
    </Card>
  )
}
