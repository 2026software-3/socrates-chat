import type { ComponentProps, ReactElement } from 'react'
import { cn } from 'cn'
import { Button as ShadcnButton } from '@/components/ui/button'

type ButtonProps = Omit<ComponentProps<typeof ShadcnButton>, 'asChild' | 'children' | 'disabled' | 'size' | 'type' | 'variant' | 'aria-label'> & (
  | { variant: 'icon'; 'aria-label': string }
  | { variant?: 'primary' | 'quiet' | 'ghost' | 'link'; 'aria-label'?: string }
) & (
  | { asChild: true; children: ReactElement; disabled?: never; type?: never }
  | { asChild?: false; children?: ComponentProps<'button'>['children']; disabled?: boolean; type?: ComponentProps<'button'>['type'] }
)

export function Button({ variant = 'primary', asChild, className, type = 'button', ...props }: ButtonProps) {
  return (
    <ShadcnButton
      {...props}
      asChild={asChild}
      type={asChild ? undefined : type}
      variant="ghost"
      data-vedute-variant={variant}
      className={cn('vedute-button', className)}
    />
  )
}
