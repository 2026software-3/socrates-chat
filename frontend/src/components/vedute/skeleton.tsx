import type { ReactNode } from 'react'
import { cn } from 'cn'
import { Skeleton as ShadcnSkeleton } from '@/components/ui/skeleton'

export function Skeleton({ className }: { className?: string }) {
  return <ShadcnSkeleton aria-hidden="true" className={cn('vedute-skeleton', className)} />
}

export function SkeletonRegion({
  loadingLabel, className, children,
}: { loadingLabel: string; className?: string; children: ReactNode }) {
  return (
    <>
      <span className="sr-only" role="status">{loadingLabel}</span>
      <div aria-busy="true" className={className}>{children}</div>
    </>
  )
}
