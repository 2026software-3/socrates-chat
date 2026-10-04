import { Badge } from '@/components/ui/badge'
import { frameworkLabelKey } from '@/features/dashboard/labels'
import { dictionaries, useT } from '@/i18n'
import { FRAMEWORKS } from '@/lib/types'

/**
 * 總結上的「主要學派」標籤（AI 標記，暫定；只是輔助資訊，主張才是論點本身）。沒有時不顯示；
 * 管理者看到的是固定字串 `message`（S-02.2），原樣顯示，不翻譯。
 */
export function FrameworkBadge({ framework }: { framework?: string | null }) {
  const t = useT()
  if (!framework) return null
  const known = (FRAMEWORKS as readonly string[]).includes(framework) && frameworkLabelKey(framework) in dictionaries['zh-TW']
  return <Badge variant="outline">{known ? t(frameworkLabelKey(framework)) : framework}</Badge>
}
