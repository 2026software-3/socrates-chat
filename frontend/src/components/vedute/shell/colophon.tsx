import { Link } from 'react-router'
import { toast } from 'sonner'
import { cn } from 'cn'
import { LANG_LABELS } from '@/i18n/define'
import { useI18n } from '@/i18n'

export type ColophonProps = {
  /** Shows the 「回報問題」 (`/complaint`) link for roles with reporter access (student & teacher). */
  showReportLink?: boolean
  className?: string
}

const COLOPHON_ACTION_CLASS =
  'type-note inline-flex min-h-11 cursor-pointer items-center border-0 bg-transparent p-0 text-ink no-underline hover:text-chalk hover:underline hover:decoration-chalk hover:underline-offset-4 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-chalk'

/**
 * Work-shell footer across the full content column (Figma Shell/Colophon 2027:380):
 * Desktop (≥1024): horizontal row with 40px padding; Tablet & Mobile (<1024): 2-row stack with 16px padding.
 * Omitted in Focus shell (conversation, transcript, summary).
 */
export function Colophon({ showReportLink = false, className }: ColophonProps) {
  const { lang, t } = useI18n()

  function handleComingSoon() {
    toast.info(t('shell.colophon.comingSoon'))
  }

  return (
    <footer
      className={cn(
        'vedute-colophon mt-auto flex flex-col gap-3.5 bg-paper px-4 pt-5 pb-7 lg:px-10',
        className,
      )}
    >
      <div aria-hidden="true" className="h-1 w-full border-t border-b border-ink" />
      <div className="flex flex-col items-start gap-2.5 lg:flex-row lg:items-baseline lg:justify-between">
        <p className="flex flex-wrap items-baseline gap-x-3 gap-y-1 text-ink-soft">
          <span className="vedute-colophon-latin">SCHOLA SOCRATICA · MMXXVI</span>
          <span className="type-note">{t('shell.brand')}</span>
        </p>

        <nav aria-label={t('shell.colophon.nav')} className="flex flex-wrap items-center gap-x-5 gap-y-1">
          <button type="button" onClick={handleComingSoon} className={COLOPHON_ACTION_CLASS}>
            {t('shell.colophon.guide')}
          </button>
          <button type="button" onClick={handleComingSoon} className={COLOPHON_ACTION_CLASS}>
            {t('shell.colophon.privacy')}
          </button>
          {showReportLink && (
            <Link to="/complaint" className={COLOPHON_ACTION_CLASS}>
              {t('complaints.nav')}
            </Link>
          )}
          <span className="type-note inline-flex min-h-11 items-center text-ink">
            {t('shell.colophon.language', { lang: LANG_LABELS[lang] })}
          </span>
        </nav>
      </div>
    </footer>
  )
}
