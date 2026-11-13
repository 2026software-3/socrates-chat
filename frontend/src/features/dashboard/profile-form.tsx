import { useState, type FormEvent } from 'react'
import { useSearchParams } from 'react-router'
import { useAuth } from '@/auth/auth-context'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button, buttonVariants } from '@/components/ui/button'
import { Card, CardContent } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { dictionaries, errorText, useT, type MessageKey } from '@/i18n'
import { api } from '@/lib/api'
import { usePasswordChange } from '@/features/dashboard/use-password-change'
import type { Me } from '@/lib/types'

/** 顯示名稱的長度上限；後端才是準則（S-08.2），這裡只是方便使用者。 */
const DISPLAY_NAME_MAX = 30

/** 與後端一樣以 Unicode 字元計（emoji 算 1，不像 maxLength 以 UTF-16 計），去頭尾空白後再算。 */
const nameLength = (value: string) => Array.from(value.trim()).length

/**
 * 學生的個人設定：顯示名稱（留空會清除，介面回到顯示電子郵件）、Google 帳號，
 * 以及「儲存」旁的「更改密碼」按鈕（S-18）；不能改密碼的帳號在同一位置顯示說明。彈出視窗放在表單外面，見 `ChangePasswordDialog`。
 */
export function ProfileForm() {
  const auth = useAuth()
  const password = usePasswordChange()
  // 登入者載入完成前不顯示（外框保證實際使用時一定已登入）
  if (auth.status !== 'authed') return null
  const canChangePassword = auth.me.can_change_password
  return (
    <>
      <Form
        me={auth.me}
        update={auth.update}
        password={canChangePassword ? { saved: password.saved, open: password.start } : null}
      />
      {auth.me.roles.student ? <GoogleLink me={auth.me} update={auth.update} /> : null}
      {canChangePassword ? password.dialog : null}
    </>
  )
}

/** 後端連結流程結束後導回 `/account?link=ok` 或 `?link_error=<code>`。 */
function linkResultKey(params: URLSearchParams): MessageKey | null {
  if (params.get('link') === 'ok') return 'profile.google.linked'
  const code = params.get('link_error')
  if (!code) return null
  const key = `profile.google.error.${code}` as MessageKey
  return dictionaries['zh-TW'][key] ? key : 'profile.google.error.link_failed'
}

/**
 * 學生連結 Google 帳號：信箱由 Google 證明是本人的才會寫入（後端檢查），之後用 Google 登入會回到這個帳號。
 * 連結與改連結都是整頁導向，由後端與 Google 完成授權；取消連結要再按一次確認。
 */
function GoogleLink({ me, update }: { me: Me; update: (me: Me) => void }) {
  const t = useT()
  const [params] = useSearchParams()
  const resultKey = linkResultKey(params)
  const [confirming, setConfirming] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<unknown>(null)

  async function unlink() {
    setBusy(true)
    setError(null)
    try {
      await api.delete('/api/me/google-link')
      update({ ...me, email: null })
      setConfirming(false)
    } catch (err) {
      setError(err)
    } finally {
      setBusy(false)
    }
  }

  const linkClass = buttonVariants({ variant: me.email ? 'outline' : 'default' })
  return (
    <Card>
      <CardContent>
        <section aria-labelledby="google-link-title" className="max-w-sm space-y-3">
          <h2 id="google-link-title" className="text-lg font-medium">
            {t('profile.google.title')}
          </h2>
          {resultKey ? (
            <Alert variant={resultKey === 'profile.google.linked' ? 'default' : 'destructive'} role="status">
              <AlertTitle>{t(resultKey)}</AlertTitle>
            </Alert>
          ) : null}
          {error ? (
            <Alert variant="destructive" role="alert">
              <AlertTitle>{errorText(t, error)}</AlertTitle>
            </Alert>
          ) : null}
          {me.email ? (
            <p className="text-sm">
              {t('profile.google.current')} <strong>{me.email}</strong>
            </p>
          ) : (
            <p className="text-sm">{t('profile.google.none')}</p>
          )}
          <p className="text-muted-foreground text-xs">{t('profile.google.hint')}</p>
          <div className="flex flex-wrap items-center gap-2">
            {/* 整頁導向：授權由後端與 Google 完成，前端不處理任何 token */}
            <a href="/api/auth/google/link" className={linkClass}>
              {t(me.email ? 'profile.google.change' : 'profile.google.link')}
            </a>
            {me.email && !confirming ? (
              <Button type="button" variant="ghost" onClick={() => setConfirming(true)}>
                {t('profile.google.unlink')}
              </Button>
            ) : null}
            {me.email && confirming ? (
              <>
                <Button type="button" variant="destructive" disabled={busy} onClick={() => void unlink()}>
                  {t('profile.google.unlinkConfirm')}
                </Button>
                <Button type="button" variant="ghost" onClick={() => setConfirming(false)}>
                  {t('profile.google.cancel')}
                </Button>
              </>
            ) : null}
          </div>
          {confirming ? <p className="text-muted-foreground text-xs">{t('profile.google.unlinkWarn')}</p> : null}
        </section>
      </CardContent>
    </Card>
  )
}

function Form({ me, update, password }: { me: Me; update: (me: Me) => void; password: { saved: boolean; open: () => void } | null }) {
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
          <div className="flex flex-wrap items-center gap-3">
            <Button type="submit" disabled={saving || tooLong}>
              {t('profile.save')}
            </Button>
            {password ? (
              <Button type="button" variant="outline" onClick={password.open}>
                {t('profile.password.title')}
              </Button>
            ) : (
              // 不能改密碼的帳號（只用 Google 登入，或示範帳號）：在按鈕本來的位置說明，免得以為功能壞了；只是提示，不是錯誤
              <p className="text-muted-foreground text-sm">{t('account.unavailable')}</p>
            )}
            {saved ? (
              <p role="status" className="text-sm">
                {t('profile.saved')}
              </p>
            ) : null}
            {password?.saved ? (
              <p role="status" className="text-sm">
                {t('profile.password.saved')}
              </p>
            ) : null}
          </div>
        </form>
      </CardContent>
    </Card>
  )
}
