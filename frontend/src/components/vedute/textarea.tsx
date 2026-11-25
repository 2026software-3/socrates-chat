import type { ComponentProps } from 'react'
import { cn } from 'cn'
import { Textarea as ShadcnTextarea } from '@/components/ui/textarea'
import { FieldLayout, type FieldLabels } from './field-layout'

type TextareaProps = FieldLabels & ComponentProps<typeof ShadcnTextarea>

export function Textarea({
  id, label, description, error, className, 'aria-describedby': describedBy, ...props
}: TextareaProps) {
  return (
    <FieldLayout id={id} label={label} description={description} error={error} describedBy={describedBy}>
      {(attributes) => (
        <ShadcnTextarea
          rows={3}
          {...props}
          {...attributes}
          aria-invalid={attributes['aria-invalid'] ?? props['aria-invalid']}
          className={cn('vedute-input', className)}
        />
      )}
    </FieldLayout>
  )
}
