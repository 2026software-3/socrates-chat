import type { ComponentProps } from 'react'
import { cn } from 'cn'
import { Input } from '@/components/ui/input'
import { FieldLayout, type FieldLabels } from './field-layout'

type FieldProps = FieldLabels & ComponentProps<typeof Input>

export function Field({
  id, label, description, error, className, 'aria-describedby': describedBy, ...props
}: FieldProps) {
  return (
    <FieldLayout id={id} label={label} description={description} error={error} describedBy={describedBy}>
      {(attributes) => (
        <Input
          {...props}
          {...attributes}
          aria-invalid={attributes['aria-invalid'] ?? props['aria-invalid']}
          className={cn('vedute-input', className)}
        />
      )}
    </FieldLayout>
  )
}
