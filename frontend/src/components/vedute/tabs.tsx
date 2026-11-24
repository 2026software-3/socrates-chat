import type { ComponentProps } from 'react'
import { cn } from 'cn'
import {
  Tabs as ShadcnTabs,
  TabsContent as ShadcnTabsContent,
  TabsList as ShadcnTabsList,
  TabsTrigger as ShadcnTabsTrigger,
} from '@/components/ui/tabs'

export function Tabs(props: Omit<ComponentProps<typeof ShadcnTabs>, 'orientation' | 'asChild'>) {
  return <ShadcnTabs {...props} />
}

export function TabsList({ className, ...props }: Omit<ComponentProps<typeof ShadcnTabsList>, 'variant' | 'asChild'>) {
  return <ShadcnTabsList {...props} variant="line" className={cn('vedute-tabs-list', className)} />
}

export function TabsTrigger({ className, ...props }: Omit<ComponentProps<typeof ShadcnTabsTrigger>, 'asChild'>) {
  return <ShadcnTabsTrigger {...props} className={cn('vedute-tab', className)} />
}

export function TabsContent({ className, ...props }: Omit<ComponentProps<typeof ShadcnTabsContent>, 'asChild'>) {
  return <ShadcnTabsContent {...props} className={cn('vedute-tab-content', className)} />
}
