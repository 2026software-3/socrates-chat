import { useEffect, useRef, useState, type ComponentProps, type ReactElement } from 'react'
import { cn } from 'cn'
import { Button as ShadcnButton } from '@/components/ui/button'

type ButtonProps = Omit<ComponentProps<typeof ShadcnButton>, 'asChild' | 'children' | 'disabled' | 'size' | 'type' | 'variant' | 'aria-label'> & (
  | { variant: 'icon'; 'aria-label': string }
  | { variant?: 'primary' | 'quiet' | 'ghost' | 'link'; 'aria-label'?: string }
) & (
  | { asChild: true; children: ReactElement; disabled?: never; type?: never }
  | { asChild?: false; children?: ComponentProps<'button'>['children']; disabled?: boolean; type?: ComponentProps<'button'>['type'] }
)

export function Button({ variant = 'primary', asChild, className, type = 'button', onClick, ...props }: ButtonProps) {
  const [pressed, setPressed] = useState(false)
  const releaseTimer = useRef<number | undefined>(undefined)

  useEffect(() => () => window.clearTimeout(releaseTimer.current), [])

  const handleClick: ComponentProps<'button'>['onClick'] = (event) => {
    if ((variant === 'quiet' || variant === 'ghost') && !event.currentTarget.classList.contains('vedute-page')) {
      window.clearTimeout(releaseTimer.current)
      setPressed(true)
      releaseTimer.current = window.setTimeout(() => setPressed(false), 150)
    }
    onClick?.(event)
  }

  return (
    <ShadcnButton
      {...props}
      asChild={asChild}
      type={asChild ? undefined : type}
      variant="ghost"
      data-vedute-variant={variant}
      data-pressed={pressed ? '' : undefined}
      onClick={handleClick}
      className={cn('vedute-button', className)}
    />
  )
}
