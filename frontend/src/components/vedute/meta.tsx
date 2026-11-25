import type { LucideIcon } from 'lucide-react'
import type { ReactNode } from 'react'

type MetaProps = { icon: LucideIcon; value: ReactNode } & (
  | { variant?: 'field'; label: string }
  | { variant: 'bibliographic'; label?: string }
)

export function Meta({ icon: Icon, label, value, variant = 'field' }: MetaProps) {
  return (
    <span className="type-note inline-flex min-w-0 items-baseline gap-1 text-ink-soft">
      <Icon className="shrink-0 self-center" size={variant === 'bibliographic' ? 14 : 16}
        strokeWidth={1.5} absoluteStrokeWidth aria-hidden="true" />
      {label && <span className="shrink-0">{label}</span>}
      <span className="min-w-0 wrap-break-word">{value}</span>
    </span>
  )
}
