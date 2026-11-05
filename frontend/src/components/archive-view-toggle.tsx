import { Button } from '@/components/ui/button'
import { useT } from '@/i18n'

/** 清單的「使用中／已封存」檢視切換。 */
export function ArchiveViewToggle({ archived, onChange }: { archived: boolean; onChange: (archived: boolean) => void }) {
  const t = useT()
  return (
    <div role="group" aria-label={t('archiveView.label')} className="flex gap-2">
      <Button type="button" size="sm" variant={archived ? 'outline' : 'default'} aria-pressed={!archived} onClick={() => onChange(false)}>
        {t('archiveView.active')}
      </Button>
      <Button type="button" size="sm" variant={archived ? 'default' : 'outline'} aria-pressed={archived} onClick={() => onChange(true)}>
        {t('archiveView.archived')}
      </Button>
    </div>
  )
}
