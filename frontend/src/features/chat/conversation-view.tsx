import { AssistantRuntimeProvider, useAuiState, useLocalRuntime, type ThreadMessageLike } from '@assistant-ui/react'
import { useEffect, useMemo, useRef, useState } from 'react'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { createChatAdapter } from '@/features/chat/chat-adapter'
import { useMessageSource } from '@/features/chat/use-message-source'
import type { RecognitionSource } from '@/features/chat/voice/recognizer'
import { ChatThread } from '@/features/chat/chat-thread'
import { ConfirmDialog } from '@/features/chat/confirm-dialog'
import { StageIndicator } from '@/features/chat/stage-indicator'
import { SummaryPanel } from '@/features/chat/summary-panel'
import { useT } from '@/i18n'
import { api } from '@/lib/api'
import type { ConversationDetail, ConversationStatus } from '@/lib/types'

export type SummaryTiming = { pollMs?: number; maxPollMs?: number }

/** 後端訊息 → assistant-ui 的初始訊息（只取文字；question_type 等中繼資料不進畫面）。 */
function toInitialMessages(detail: ConversationDetail): ThreadMessageLike[] {
  return detail.messages.map((m) => ({
    id: m.id,
    role: m.role === 'student' ? 'user' : 'assistant',
    content: [{ type: 'text', text: m.content }],
    createdAt: new Date(m.created_at),
    ...(m.role === 'ai' ? { status: { type: 'complete', reason: 'stop' } as const } : {}),
  }))
}

/** 單一對話：標題與階段、對話串、結束討論與總結。`detail` 只在載入時使用，之後狀態由本元件維護。 */
export function ConversationView({ detail, pollMs, maxPollMs }: { detail: ConversationDetail } & SummaryTiming) {
  const t = useT()
  const [status, setStatus] = useState<ConversationStatus>(detail.status)
  const [stage, setStage] = useState(detail.stage)
  const [suggestEnd, setSuggestEnd] = useState(detail.converge_ready)
  const [ending, setEnding] = useState(false)
  const [endError, setEndError] = useState<unknown>()

  // 下一則學生訊息的輸入來源：語音輸入後送出時記錄為語音（S-05.4），送出後回到文字
  const messageSource = useMessageSource()

  const adapter = useMemo(
    () =>
      createChatAdapter({
        conversationId: detail.id,
        takeSource: messageSource.take,
        persistedIds: detail.messages.filter((m) => m.role === 'student').map((m) => m.id),
        // 達回合上限：對話已自動結束、總結產生中，不會有串流
        onSent: (sent) => {
          if (sent.auto_ended) setStatus('ended')
        },
        onEnded: () => setStatus('ended'),
        onDone: (done) => {
          setStage(done.stage)
          setSuggestEnd(done.suggest_end)
        },
      }),
    [detail, messageSource],
  )
  const initialMessages = useMemo(() => toInitialMessages(detail), [detail])
  const runtime = useLocalRuntime(adapter, { initialMessages })

  // 離開對話頁時回覆還在產生：回來後直接接上（只重開串流），不必按重試
  const resumed = useRef(false)
  useEffect(() => {
    const last = detail.messages.at(-1)
    if (resumed.current || !detail.generating || last?.role !== 'student') return
    resumed.current = true
    runtime.thread.startRun({ parentId: last.id })
  }, [detail, runtime])

  const ended = status === 'ended'

  async function endDiscussion() {
    if (ending) return
    setEnding(true)
    setEndError(undefined)
    try {
      // 202 開始產生總結；已結束時為 200
      await api.post(`/api/conversations/${detail.id}/end`)
      setStatus('ended')
    } catch (e) {
      setEndError(e)
    } finally {
      setEnding(false)
    }
  }

  return (
    <AssistantRuntimeProvider runtime={runtime}>
      <div className="flex min-h-0 flex-1 flex-col">
        <header className="flex flex-wrap items-end justify-between gap-4 border-b px-4 pt-6 pb-4 md:px-8">
          <div className="flex min-w-0 flex-col gap-1">
            <div className="flex flex-wrap items-center gap-2">
              <h1 className="text-2xl leading-8 font-medium break-words">{detail.title}</h1>
              <Badge variant={ended ? 'secondary' : 'default'}>{t(`chat.status.${status}`)}</Badge>
            </div>
            {detail.description ? <p className="text-muted-foreground text-xs break-words">{detail.description}</p> : null}
            <div className="mt-1 flex flex-wrap items-center gap-2">
              <RoundCount />
              <StageIndicator stage={stage} />
            </div>
          </div>
          <EndDiscussion
            ended={ended}
            suggestEnd={suggestEnd}
            pending={ending}
            error={endError}
            onClearError={() => setEndError(undefined)}
            onConfirm={endDiscussion}
          />
        </header>

        <ChatThread
          showComposer={!ended}
          composerDisabled={ending}
          language={detail.language}
          topicId={detail.topic_id}
          onVoiceSource={(s: RecognitionSource) => messageSource.set(s)}
        />
        {ended ? (
          <div className="space-y-4 px-4 pb-6 md:px-8">
            <p className="text-muted-foreground text-sm">{t('chat.ended.readonly')}</p>
            <SummaryPanel conversationId={detail.id} pollMs={pollMs} maxPollMs={maxPollMs} />
          </div>
        ) : null}
      </div>
    </AssistantRuntimeProvider>
  )
}

/** 回合數：學生送出幾則訊息就是第幾回合；跟著對話串即時更新。 */
function RoundCount() {
  const t = useT()
  const count = useAuiState((s) => s.thread.messages.filter((m) => m.role === 'user').length)
  return <span className="text-xs font-medium">{t('chat.round', { count })}</span>
}

type EndProps = {
  ended: boolean
  suggestEnd: boolean
  pending: boolean
  error: unknown
  onClearError: () => void
  onConfirm: () => Promise<void>
}

/** 結束討論：按鈕（AI 回覆中停用）＋確認對話框＋建議收斂的提示。 */
function EndDiscussion({ ended, suggestEnd, pending, error, onClearError, onConfirm }: EndProps) {
  const t = useT()
  const running = useAuiState((s) => s.thread.isRunning)
  const [open, setOpen] = useState(false)

  if (ended) return null

  const endButton = (
    <Button
      variant={suggestEnd ? 'default' : 'outline'}
      disabled={running || pending}
      onClick={() => {
        onClearError()
        setOpen(true)
      }}
    >
      {pending ? t('chat.end.ending') : t('chat.end.button')}
    </Button>
  )

  return (
    <>
      {suggestEnd ? (
        <Alert role="status" data-testid="suggest-end" className="w-full md:w-auto md:max-w-sm">
          <AlertTitle>{t('chat.suggestEnd.title')}</AlertTitle>
          <AlertDescription>
            <p>{t('chat.suggestEnd.body')}</p>
            <div className="mt-2">{endButton}</div>
          </AlertDescription>
        </Alert>
      ) : (
        endButton
      )}
      <ConfirmDialog
        open={open}
        onOpenChange={setOpen}
        title={t('chat.end.confirmTitle')}
        description={t('chat.end.confirmBody')}
        confirmLabel={t('chat.end.confirm')}
        pending={pending}
        error={error}
        // 成功後對話變成 ended，整個元件卸載；失敗則保持開啟並顯示錯誤
        onConfirm={() => void onConfirm()}
      />
    </>
  )
}
