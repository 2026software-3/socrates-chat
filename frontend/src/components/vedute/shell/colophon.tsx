import { Link } from 'react-router'
import { cn } from 'cn'
import { LANG_LABELS } from '@/i18n/define'
import { useI18n } from '@/i18n'

export type ColophonProps = {
  /** Shows the 「回報問題」 (`/complaint`) link for roles with reporter access (student & teacher). */
  showReportLink?: boolean
  className?: string
}

/**
 * Work-shell footer across the full content column (Figma Shell/Colophon 2027:380).
 * Omitted in Focus shell (conversation, transcript, summary).
 */
export function Colophon({ showReportLink = false, className }: ColophonProps) {
  const { lang, t } = useI18n()

  return (
    <footer
      className={cn(
        'mt-auto flex flex-col gap-3.5 bg-paper px-4 pt-5 pb-7 lg:px-10',
        className,
      )}
    >
      <div aria-hidden="true" className="h-1 w-full border-t border-b border-ink" />
      <div className="flex flex-col items-start gap-2.5 md:flex-row md:items-baseline md:justify-between">
        <p className="flex flex-wrap items-baseline gap-x-3 gap-y-1 text-ink-soft">
          <span className="type-latin-small">SCHOLA SOCRATICA · MMXXVI</span>
          <span className="type-note">{t('shell.brand')}</span>
        </p>

        <nav aria-label={t('shell.colophon.nav')} className="flex flex-wrap items-center gap-x-5 gap-y-1">
          {showReportLink && (
            <Link
              to="/complaint"
              className="type-note inline-flex min-h-11 items-center text-ink underline decoration-chalk underline-offset-4 hover:text-chalk focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-chalk"
              style={{ textDecorationThickness: 'var(--stroke-emphasis)' }}
            >
              {t('complaints.nav')}
            </Link>
          )}
          <span className="type-note text-ink-soft">
            {t('shell.colophon.language', { lang: LANG_LABELS[lang] })}
          </span>
        </nav>
      </div>
    </footer>
  )
}
