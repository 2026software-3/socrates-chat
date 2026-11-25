import { useId, type ReactNode } from 'react'
import { Label } from '@/components/ui/label'

export type FieldLabels = {
  label: ReactNode
  description?: string
  error?: string
}

type ControlAttributes = {
  id: string
  'aria-describedby'?: string
  'aria-invalid'?: boolean
}

export function FieldLayout({
  id, label, description, error, describedBy, children,
}: FieldLabels & {
  id?: string
  describedBy?: string
  children: (attributes: ControlAttributes) => ReactNode
}) {
  const fieldId = useId()
  const controlId = id ?? fieldId
  const descriptionId = description ? `${fieldId}-description` : undefined
  const errorId = error ? `${fieldId}-error` : undefined
  const descriptionIds = [describedBy, descriptionId, errorId].filter(Boolean).join(' ')

  return (
    <div className="grid min-w-0 gap-1">
      <Label htmlFor={controlId} className="vedute-label type-note text-ink-soft">{label}</Label>
      {children({
        id: controlId,
        'aria-describedby': descriptionIds || undefined,
        'aria-invalid': error ? true : undefined,
      })}
      {description && <p id={descriptionId} className="type-note text-ink-soft">{description}</p>}
      {error && <p id={errorId} className="type-note text-error">{error}</p>}
    </div>
  )
}
