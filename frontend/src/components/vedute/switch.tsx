import { useId, type ComponentProps } from 'react'
import { cn } from 'cn'
import { Lock } from 'lucide-react'
import { Label } from '@/components/ui/label'
import { Switch as ShadcnSwitch } from '@/components/ui/switch'

type SwitchProps = Omit<ComponentProps<typeof ShadcnSwitch>, 'size' | 'children' | 'asChild'> & {
  label: string
  locked?: boolean
}

export function Switch({ id, label, locked, checked, defaultChecked, disabled, className, ...props }: SwitchProps) {
  const generatedId = useId()
  const controlId = id ?? generatedId
  return (
    <div className={cn('flex min-h-11 items-center gap-2', className)}>
      <span className="relative inline-flex shrink-0 items-center">
        <ShadcnSwitch
          {...props}
          key={locked ? 'locked' : 'switch'}
          id={controlId}
          className="vedute-switch"
          data-locked={locked || undefined}
          checked={locked ? false : checked}
          defaultChecked={locked ? undefined : defaultChecked}
          disabled={locked || disabled}
        />
        {locked && <Lock className="pointer-events-none absolute left-2 size-4 text-ink" aria-hidden="true" />}
      </span>
      <Label htmlFor={controlId} className="vedute-label type-switch-label text-ink">{label}</Label>
    </div>
  )
}
