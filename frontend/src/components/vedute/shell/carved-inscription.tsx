import type { CSSProperties } from 'react'
import { cn } from 'cn'

export type CarvedInscriptionProps = {
  /** Latin building name, e.g. "Basilica Ivlia". Words are joined by an interpunct (·). */
  text: string
  /** Delay in ms before the first glyph starts chiseling (0 for Topbar/BackHeader, 380 for PageHeader). */
  delayMs?: number
  /** Changes only on area arrival so same-page updates do not replay the chisel animation. */
  animateKey?: string
  variant?: 'topbar' | 'header' | 'back'
  className?: string
}

/**
 * Roman inscription carved letter by letter on arrival (prototypes/c-interior/index.html `carve`).
 * Always decorative (`aria-hidden="true"`) since the localized area label provides the accessible name.
 */
export function CarvedInscription({
  text,
  delayMs = 0,
  animateKey,
  variant = 'topbar',
  className,
}: CarvedInscriptionProps) {
  const words = text.trim().toUpperCase().split(/\s+/).filter(Boolean)
  let glyphIndex = 0

  return (
    <span
      key={animateKey}
      data-vedute-inscription=""
      aria-hidden="true"
      style={{ '--chisel-delay': `${delayMs}ms` } as CSSProperties}
      className={cn(
        'vedute-inscription',
        variant === 'topbar' && 'vedute-inscription-topbar',
        variant === 'header' && 'vedute-inscription-header',
        variant === 'back' && 'vedute-inscription-back',
        className,
      )}
    >
      {words.map((word, wordIndex) => {
        const dotIndex = wordIndex > 0 ? glyphIndex++ : -1
        const chars = [...word].map((ch) => ({ ch, idx: glyphIndex++ }))
        return (
          <span key={`${word}-${wordIndex}`}>
            {wordIndex > 0 && (
              <span data-glyph="" data-dot="" style={{ '--i': dotIndex } as CSSProperties}>
                ·
              </span>
            )}
            {chars.map(({ ch, idx }) => (
              <span key={idx} data-glyph="" style={{ '--i': idx } as CSSProperties}>
                {ch}
              </span>
            ))}
          </span>
        )
      })}
    </span>
  )
}
