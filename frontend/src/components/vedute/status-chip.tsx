import { cn } from 'cn'
import { Badge } from '@/components/ui/badge'

const tones = {
  active: 'border-aerugo bg-aerugo-wash text-aerugo',
  draft: 'border-ochra bg-ochra-wash text-ochra',
  inactive: 'border-ink-faint bg-transparent text-ink-soft',
}

export function StatusChip({ tone, children }: { tone: keyof typeof tones; children: string }) {
  return <Badge variant="outline" className={cn('vedute-status-chip', tones[tone])}>{children}</Badge>
}
