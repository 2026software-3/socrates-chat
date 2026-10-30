import { useAui, useAuiState } from '@assistant-ui/react'
import { useEffect, useState } from 'react'
import { Button } from '@/components/ui/button'
import { DEMO_EXTRA_REPLIES, DEMO_REPLIES, DEMO_TOPIC_ID } from '@/features/chat/demo-replies-data'
import { useT } from '@/i18n'
import { api } from '@/lib/api'

/** 後端有開 `DEMO_MODE` 才有 `/api/demo/accounts`；沒開是 404，快速回覆就不出現。 */
function useDemoMode(): boolean {
  const [on, setOn] = useState(false)
  useEffect(() => {
    const controller = new AbortController()
    api
      .get('/api/demo/accounts', controller.signal)
      .then(() => setOn(true))
      .catch(() => {})
    return () => controller.abort()
  }, [])
  return on
}

/**
 * 示範模式的快速回覆（發表用）：只在示範模式、且是「自由意志」示範題時出現。
 * 點一則只會把文字填進輸入框，不會自己送出；送出仍走一般流程。
 */
export function DemoReplies({ topicId, disabled }: { topicId: string | null; disabled: boolean }) {
  const t = useT()
  const aui = useAui()
  const demo = useDemoMode()
  // 已送出幾則學生訊息，就推薦下一則
  const sent = useAuiState((s) => s.thread.messages.filter((m) => m.role === 'user').length)
  if (!demo || topicId !== DEMO_TOPIC_ID) return null

  const fill = (text: string) => aui.composer().setText(text)
  const next = Math.min(sent, DEMO_REPLIES.length - 1)
  return (
    <section
      aria-labelledby="demo-replies-title"
      className="border-border flex flex-col gap-2 rounded-md border border-dashed p-3"
    >
      <div>
        <h2 id="demo-replies-title" className="text-sm font-medium">
          {t('chat.demo.title')}
        </h2>
        <p className="text-muted-foreground text-xs">{t('chat.demo.hint')}</p>
      </div>
      <Button
        type="button"
        size="sm"
        className="h-auto justify-start py-1.5 text-left whitespace-normal"
        disabled={disabled}
        onClick={() => fill(DEMO_REPLIES[next]!)}
      >
        <span className="font-medium">{t('chat.demo.next', { n: next + 1 })}</span> {DEMO_REPLIES[next]}
      </Button>
      <details className="text-xs">
        <summary className="text-muted-foreground cursor-pointer">{t('chat.demo.all')}</summary>
        <ol className="mt-2 flex flex-col gap-1.5">
          {DEMO_REPLIES.map((text, i) => (
            <li key={text}>
              <Button
                type="button"
                variant="secondary"
                size="sm"
                className="h-auto w-full justify-start py-1.5 text-left whitespace-normal"
                disabled={disabled}
                onClick={() => fill(text)}
              >
                {i + 1}. {text}
              </Button>
            </li>
          ))}
          {DEMO_EXTRA_REPLIES.map((r) => (
            <li key={r.key}>
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-auto w-full justify-start py-1.5 text-left whitespace-normal"
                disabled={disabled}
                onClick={() => fill(r.text)}
              >
                {t(r.key)}：{r.text}
              </Button>
            </li>
          ))}
        </ol>
      </details>
    </section>
  )
}
