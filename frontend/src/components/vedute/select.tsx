import type { ComponentProps } from 'react'
import { cn } from 'cn'
import { Select as SelectPrimitive } from 'radix-ui'
import { Check, ChevronDown, ChevronUp } from 'lucide-react'
import { FieldLayout, type FieldLabels } from './field-layout'

type SelectOption = {
  value: string
  label: string
  disabled?: boolean
  /** 不能選的原因（例如「已封存」），顯示在選項右側，不只靠顏色表達。 */
  hint?: string
}

type SelectProps = FieldLabels & Omit<ComponentProps<typeof SelectPrimitive.Root>, 'children'> & {
  id?: string
  className?: string
  placeholder?: string
  options: readonly SelectOption[]
  'aria-describedby'?: string
}

/**
 * Vedute 下拉選單：觸發鈕是「紙上橫線」欄位；選單是壓在紙上的一張紙條（紙底、墨框、內側版痕線，不加投影），
 * 目前指到的選項用深紙底＋左側紅粉筆線，已選的選項用粗體＋右側墨色勾。
 * 直接組 radix 的 Select，不改 shadcn 的 `components/ui/select`。
 */
export function Select({
  id, label, description, error, className, placeholder, options, disabled,
  'aria-describedby': describedBy, ...props
}: SelectProps) {
  return (
    <FieldLayout id={id} label={label} description={description} error={error} describedBy={describedBy}>
      {(attributes) => (
        <SelectPrimitive.Root {...props} disabled={disabled || options.length === 0}>
          <SelectPrimitive.Trigger {...attributes} data-slot="select-trigger"
            className={cn('vedute-input vedute-select-trigger', className)}>
            <SelectPrimitive.Value data-slot="select-value" placeholder={placeholder} />
            <SelectPrimitive.Icon asChild>
              <ChevronDown className="vedute-select-chevron" aria-hidden="true" />
            </SelectPrimitive.Icon>
          </SelectPrimitive.Trigger>
          <SelectPrimitive.Portal>
            <SelectPrimitive.Content position="popper" align="start" sideOffset={6} className="vedute-select-content">
              <SelectPrimitive.ScrollUpButton className="vedute-select-scroll">
                <ChevronUp className="size-4" aria-hidden="true" />
              </SelectPrimitive.ScrollUpButton>
              <SelectPrimitive.Viewport className="vedute-select-viewport">
                {options.map((option) => (
                  <SelectPrimitive.Item key={option.value} value={option.value} disabled={option.disabled}
                    className="vedute-select-item">
                    <SelectPrimitive.ItemText>{option.label}</SelectPrimitive.ItemText>
                    {option.hint ? <span className="vedute-select-hint">{option.hint}</span> : null}
                    <SelectPrimitive.ItemIndicator className="vedute-select-check" data-vedute-check="">
                      <Check className="size-4" aria-hidden="true" />
                    </SelectPrimitive.ItemIndicator>
                  </SelectPrimitive.Item>
                ))}
              </SelectPrimitive.Viewport>
              <SelectPrimitive.ScrollDownButton className="vedute-select-scroll">
                <ChevronDown className="size-4" aria-hidden="true" />
              </SelectPrimitive.ScrollDownButton>
            </SelectPrimitive.Content>
          </SelectPrimitive.Portal>
        </SelectPrimitive.Root>
      )}
    </FieldLayout>
  )
}
