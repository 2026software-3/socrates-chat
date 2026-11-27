import { useEffect, useId, useRef, type RefObject } from 'react'
import { Link } from 'react-router'
import { LANGS, LANG_LABELS, type Lang } from '@/i18n/define'
import { useI18n } from '@/i18n'
import type { Me } from '@/lib/types'
import { availableSwitchRoles, ROLE_HOME, type ShellRole } from './areas'

export type AccountMenuProps = {
  open: boolean
  me: Me
  currentRole: ShellRole
  onClose: () => void
  onSelectLanguage?: (lang: Lang) => void
  languageDisabled?: boolean
  onLogout?: () => void
  returnFocusTo?: RefObject<HTMLElement | null>
}

const WORKSPACE_LABEL_KEY = {
  student: 'shell.workspace.student',
  teacher: 'nav.workspace.teacher',
  admin: 'nav.workspace.admin',
} as const

/**
 * Account popover menu anchored below the Topbar right edge (Figma Nav/Account Menu 2063:718):
 * name, role, optional account settings link, multi-role workspace switcher, language, logout.
 */
export function AccountMenu({
  open,
  me,
  currentRole,
  onClose,
  onSelectLanguage,
  languageDisabled = false,
  onLogout,
  returnFocusTo,
}: AccountMenuProps) {
  const { lang, setLang, t } = useI18n()
  const menuRef = useRef<HTMLDivElement>(null)
  const langId = useId()

  useEffect(() => {
    if (!open) return

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        onClose()
        returnFocusTo?.current?.focus()
      }
    }
    const onPointerDown = (e: MouseEvent) => {
      const target = e.target as Node | null
      if (!target) return
      if (menuRef.current?.contains(target)) return
      if (returnFocusTo?.current?.contains(target)) return
      onClose()
    }

    document.addEventListener('keydown', onKeyDown)
    document.addEventListener('mousedown', onPointerDown)
    return () => {
      document.removeEventListener('keydown', onKeyDown)
      document.removeEventListener('mousedown', onPointerDown)
    }
  }, [open, onClose, returnFocusTo])

  if (!open) return null

  const name = me.display_name ?? me.email ?? ''
  const roleText = (['admin', 'teacher', 'student'] as const)
    .filter((r) => me.roles[r])
    .map((r) => t(`role.${r}`))
    .join(' / ')
  const switchRoles = availableSwitchRoles(me.roles, currentRole)
  const showAccountLink = me.roles.student || me.can_change_password

  function handleLanguage(next: Lang) {
    if (onSelectLanguage) {
      onSelectLanguage(next)
    } else {
      setLang(next)
    }
  }

  return (
    <div
      ref={menuRef}
      id="vedute-account-menu"
      role="region"
      aria-label={t('shell.account.menu')}
      className="absolute top-full right-0 z-40 mt-px flex w-60 flex-col gap-0.5 border border-ink bg-paper px-4 pt-3 pb-2"
    >
      <div className="mb-1.5 border-b border-ink-faint pb-2.5">
        <p className="type-body-strong truncate text-ink">{name}</p>
        <p className="type-note text-ink-soft">{roleText}</p>
      </div>

      {showAccountLink && (
        <Link
          to="/account"
          onClick={onClose}
          className="type-body flex min-h-11 items-center text-ink no-underline hover:text-chalk focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-chalk"
        >
          {t('shell.account.settings')}
        </Link>
      )}

      {switchRoles.map((targetRole) => (
        <Link
          key={targetRole}
          to={ROLE_HOME[targetRole]}
          onClick={onClose}
          className="type-body flex min-h-11 items-center text-ink no-underline hover:text-chalk focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-chalk"
        >
          {t('shell.account.switchTo', { workspace: t(WORKSPACE_LABEL_KEY[targetRole]) })}
        </Link>
      ))}

      <div className="flex min-h-11 items-center justify-between gap-2">
        <label htmlFor={langId} className="type-body shrink-0 text-ink">
          {t('nav.language')}
        </label>
        <select
          id={langId}
          value={lang}
          disabled={languageDisabled}
          onChange={(e) => handleLanguage(e.target.value as Lang)}
          className="type-note min-h-11 min-w-0 cursor-pointer border-0 border-b border-ink-faint bg-transparent px-1 text-ink focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-chalk"
        >
          {LANGS.map((code) => (
            <option key={code} value={code}>
              {LANG_LABELS[code]}
            </option>
          ))}
        </select>
      </div>

      <button
        type="button"
        onClick={() => {
          onClose()
          onLogout?.()
        }}
        className="type-body flex min-h-11 w-full cursor-pointer items-center border-0 bg-transparent p-0 text-left text-ink hover:text-chalk focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-chalk"
      >
        {t('nav.logout')}
      </button>
    </div>
  )
}
