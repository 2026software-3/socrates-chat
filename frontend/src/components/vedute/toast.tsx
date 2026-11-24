import { useEffect, useEffectEvent, useRef, useState } from 'react'
import { CircleAlert, CircleCheck, Info, X } from 'lucide-react'
import { Button } from './button'

const DURATION_MS = 4000
const icons = {
  success: CircleCheck,
  info: Info,
  error: CircleAlert,
}
const iconColors = {
  success: 'text-aerugo',
  info: 'text-caeruleum',
  error: 'text-error',
}

export type ToastNotification = {
  id: string | number
  kind: keyof typeof icons
  message: string
  action?: { label: string; onClick: () => void }
}

type ToastProps = {
  notification: ToastNotification | null
  dismissLabel: string
  onDismiss: (id: ToastNotification['id']) => void
  placement?: 'viewport' | 'anchor'
}

export function Toast({ notification, dismissLabel, onDismiss, placement = 'viewport' }: ToastProps) {
  return (
    <>
      <span className="sr-only" role="status">
        {notification?.kind !== 'error' && notification?.message}
      </span>
      <span className="sr-only" role="alert">
        {notification?.kind === 'error' && notification.message}
      </span>
      {notification && (
        <ToastCard key={notification.id} notification={notification} dismissLabel={dismissLabel}
          placement={placement} onDismiss={() => onDismiss(notification.id)} />
      )}
    </>
  )
}

function ToastCard({
  notification: { kind, message, action }, dismissLabel, placement, onDismiss,
}: {
  notification: ToastNotification
  dismissLabel: string
  placement: NonNullable<ToastProps['placement']>
  onDismiss: () => void
}) {
  const [hovered, setHovered] = useState(false)
  const [focused, setFocused] = useState(false)
  const remaining = useRef(DURATION_MS)
  const dismiss = useEffectEvent(onDismiss)
  const Icon = icons[kind]
  const paused = hovered || focused

  useEffect(() => {
    if (kind === 'error' || paused) return

    const started = performance.now()
    const timer = window.setTimeout(dismiss, remaining.current)
    return () => {
      window.clearTimeout(timer)
      remaining.current = Math.max(0, remaining.current - (performance.now() - started))
    }
  }, [kind, paused])

  return (
    <div className="vedute-toast" data-placement={placement}
      onMouseEnter={() => setHovered(true)} onMouseLeave={() => setHovered(false)}
      onFocus={() => setFocused(true)}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) setFocused(false)
      }}>
      <Icon className={`size-5 shrink-0 ${iconColors[kind]}`} aria-hidden="true" />
      <p className="min-w-0 flex-1 wrap-break-word" aria-hidden="true">{message}</p>
      {action && (
        <Button variant="link" className="vedute-toast-action" onClick={action.onClick}>{action.label}</Button>
      )}
      <Button variant="icon" aria-label={dismissLabel} onClick={onDismiss}>
        <X className="size-4" aria-hidden="true" />
      </Button>
    </div>
  )
}
