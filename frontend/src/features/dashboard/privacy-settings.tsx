import {
  BarChart2,
  Eye,
  FileText,
  Lock,
  Mail,
  MessageSquare,
  Shirt,
  User,
  type LucideIcon,
} from 'lucide-react'
import { useAuth } from '@/auth/auth-context'
import { Alert } from '@/components/vedute/alert'
import { StatusChip } from '@/components/vedute/status-chip'
import { Switch } from '@/components/vedute/switch'
import { useT, type MessageKey } from '@/i18n'

type PrivacyItem = {
  id: string
  icon: LucideIcon
  labelKey: MessageKey
  mode: 'planned' | 'locked'
}

const PRIVACY_ITEMS: readonly PrivacyItem[] = [
  { id: 'display-name', icon: User, labelKey: 'privacy.item.displayName', mode: 'planned' },
  { id: 'avatar', icon: Shirt, labelKey: 'privacy.item.avatar', mode: 'planned' },
  { id: 'email', icon: Mail, labelKey: 'privacy.item.email', mode: 'locked' },
  { id: 'stats', icon: BarChart2, labelKey: 'privacy.item.stats', mode: 'locked' },
  { id: 'conversations', icon: MessageSquare, labelKey: 'privacy.item.conversations', mode: 'locked' },
  { id: 'summaries', icon: FileText, labelKey: 'privacy.item.summaries', mode: 'locked' },
]

/**
 * 個人檔案・隱私設定（F-41.17、DESIGN.md）：
 * 公開功能尚未開放（現況）：「顯示名稱」「角色外觀」為 disabled off + 規劃中說明；
 * 「電子郵件」「統計資料」「對話內容」「學習總結」為 locked + 永不公開說明。
 */
export function PrivacySettingsSection() {
  const t = useT()
  const auth = useAuth()
  if (auth.status !== 'authed') return null

  const me = auth.me
  const name = me.display_name ?? me.email ?? ''
  const initial = [...name][0]?.toUpperCase() ?? '?'

  return (
    <div className="space-y-6">
      <Alert kind="info">{t('privacy.banner')}</Alert>

      <div className="grid grid-cols-1 gap-6 md:grid-cols-2">
        {/* 左卡：同學看得到什麼 */}
        <section
          aria-labelledby="privacy-classmates-heading"
          className="flex flex-col justify-between gap-5 rounded-sm border border-ink-faint bg-surface p-4 md:p-5"
        >
          <div className="space-y-4">
            <h2 id="privacy-classmates-heading" className="type-h3 text-ink">
              {t('privacy.classmatesTitle')}
            </h2>

            <ul className="divide-y divide-ink-faint border-t border-b border-ink-faint">
              {PRIVACY_ITEMS.map((item) => {
                const Icon = item.icon
                const isPlanned = item.mode === 'planned'
                const label = t(item.labelKey)
                return (
                  <li key={item.id} className="flex items-center justify-between gap-3 py-3">
                    <div className="flex min-w-0 flex-col gap-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <Icon className="size-4 shrink-0 text-ink-soft" strokeWidth={1.5} aria-hidden="true" />
                        <span className="type-body-strong text-ink">{label}</span>
                        {isPlanned ? (
                          <StatusChip tone="draft">{t('privacy.status.planned')}</StatusChip>
                        ) : (
                          <StatusChip tone="inactive">{t('privacy.status.neverPublic')}</StatusChip>
                        )}
                      </div>
                      <p className={isPlanned ? 'type-note text-ochra' : 'type-note text-ink-soft'}>
                        {t(isPlanned ? 'privacy.status.plannedHint' : 'privacy.status.neverPublicHint')}
                      </p>
                    </div>

                    <div className="flex min-h-11 min-w-14 shrink-0 items-center justify-end">
                      <Switch
                        label={label}
                        checked={false}
                        disabled={isPlanned}
                        locked={!isPlanned}
                        className="[&>label]:sr-only"
                      />
                    </div>
                  </li>
                )
              })}
            </ul>
          </div>

          <p className="type-note flex items-start gap-2 text-ink-soft">
            <Eye className="mt-0.5 size-4 shrink-0" strokeWidth={1.5} aria-hidden="true" />
            <span>{t('privacy.teacherNote')}</span>
          </p>
        </section>

        {/* 右卡：同學看到的樣子（預覽） */}
        <section
          aria-labelledby="privacy-preview-heading"
          className="flex flex-col gap-4 rounded-sm border border-ink-faint bg-surface p-4 md:p-5"
        >
          <h2 id="privacy-preview-heading" className="type-h3 text-ink">
            {t('privacy.previewTitle')}
          </h2>

          <div className="flex flex-col items-center gap-3 rounded-sm border border-ink-faint bg-paper p-6 text-center">
            <div
              aria-hidden="true"
              className="font-serif flex size-18 items-center justify-center rounded-full border border-ink bg-paper-deep text-2xl font-bold text-ink"
            >
              {initial}
            </div>
            <div className="space-y-1.5">
              <p className="type-h3 text-ink">{name}</p>
              <StatusChip tone="inactive">{t('role.student')}</StatusChip>
            </div>
          </div>

          <p className="type-note flex items-center gap-1.5 text-ink-soft">
            <Lock className="size-3.5 shrink-0" strokeWidth={1.5} aria-hidden="true" />
            <span>{t('privacy.previewNote')}</span>
          </p>
        </section>
      </div>
    </div>
  )
}