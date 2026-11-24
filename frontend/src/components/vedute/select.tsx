import type { ComponentProps } from 'react'
import { cn } from 'cn'
import { Select as ShadcnSelect, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { FieldLayout, type FieldLabels } from './field-layout'

type SelectProps = FieldLabels & Omit<ComponentProps<typeof ShadcnSelect>, 'children'> & {
  id?: string
  className?: string
  placeholder?: string
  options: readonly { value: string; label: string; disabled?: boolean }[]
  'aria-describedby'?: string
}

export function Select({
  id, label, description, error, className, placeholder, options, disabled,
  'aria-describedby': describedBy, ...props
}: SelectProps) {
  return (
    <FieldLayout id={id} label={label} description={description} error={error} describedBy={describedBy}>
      {(attributes) => (
        <ShadcnSelect {...props} disabled={disabled || options.length === 0}>
          <SelectTrigger {...attributes} className={cn('vedute-input', className)}>
            <SelectValue placeholder={placeholder} />
          </SelectTrigger>
          <SelectContent className="vedute-select-content">
            {options.map((option) => (
              <SelectItem key={option.value} value={option.value} disabled={option.disabled} className="vedute-select-item">
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </ShadcnSelect>
      )}
    </FieldLayout>
  )
}
