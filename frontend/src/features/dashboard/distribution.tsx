/** 數字摘要磚。 */
export function Stat({ label, value }: { label: string; value: number | string }) {
  return (
    <div className="rounded-lg border p-2.5 sm:p-4">
      <p className="text-muted-foreground text-xs sm:text-sm">{label}</p>
      <p className="text-2xl font-semibold tabular-nums sm:text-3xl">{value}</p>
    </div>
  )
}
