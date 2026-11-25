import type { ComponentProps } from 'react'
import { cn } from 'cn'
import { Alert as ShadcnAlert } from '@/components/ui/alert'

type AlertProps = Omit<ComponentProps<typeof ShadcnAlert>, 'variant' | 'role'> & { kind?: 'info' | 'error' }

export function Alert({ kind = 'info', className, ...props }: AlertProps) {
  return (
    <ShadcnAlert
      {...props}
      role={kind === 'error' ? 'alert' : 'status'}
      data-kind={kind}
      className={cn('vedute-alert', className)}
    />
  )
}
