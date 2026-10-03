import {
  ActionBarPrimitive,
  AuiIf,
  ComposerPrimitive,
  MessagePrimitive,
  ThreadPrimitive,
  useAui,
  useAuiState,
  type TextMessagePartComponent,
} from '@assistant-ui/react'
import { ArrowUp, RefreshCw } from 'lucide-react'
import { Alert, AlertTitle } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { errorText, useT } from '@/i18n'
import { ApiError } from '@/lib/api'

/** 後端單則訊息長度上限（invalid_message）。 */
const MAX_LENGTH = 4000

/**
 * 對話串。只用 assistant-ui 的 primitives（狀態、串流與重試由 runtime 處理），外觀與文字自己渲染：
 * 不顯示附件、推理、工具、編輯、分支與其他與後端契約無關的功能，也只顯示 AI 回覆文字本身。
 */
export function ChatThread({ showComposer, composerDisabled }: { showComposer: boolean; composerDisabled: boolean }) {
  const t = useT()
  return (
    <ThreadPrimitive.Root className="flex min-h-0 flex-col gap-3">
      <ThreadPrimitive.Viewport
        role="log"
        aria-live="polite"
        aria-relevant="additions text"
        aria-label={t('chat.thread.label')}
        className="flex max-h-[55vh] min-h-40 flex-col overflow-y-auto rounded-lg border p-3 sm:p-4"
      >
        <AuiIf condition={(s) => s.thread.messages.length === 0}>
          <p className="text-muted-foreground m-auto py-8 text-center text-sm">{t('chat.thread.start')}</p>
        </AuiIf>
        <div className="flex flex-col gap-4">
          <ThreadPrimitive.Messages>{() => <ChatMessage />}</ThreadPrimitive.Messages>
        </div>
      </ThreadPrimitive.Viewport>
      {showComposer ? <PendingReply /> : null}
      {showComposer ? <ChatComposer disabled={composerDisabled} /> : null}
    </ThreadPrimitive.Root>
  )
}

const PlainText: TextMessagePartComponent = ({ text }) => <p className="break-words whitespace-pre-wrap">{text}</p>

function ChatMessage() {
  const role = useAuiState((s) => s.message.role)
  return role === 'user' ? <StudentMessage /> : <AiMessage />
}

function StudentMessage() {
  const t = useT()
  return (
    <MessagePrimitive.Root data-role="student" className="flex flex-col items-end gap-1">
      <span className="text-muted-foreground text-xs">{t('chat.thread.you')}</span>
      <div className="bg-primary text-primary-foreground max-w-[85%] rounded-2xl rounded-tr-sm px-4 py-2 text-sm">
        <MessagePrimitive.Parts components={{ Text: PlainText }} />
      </div>
    </MessagePrimitive.Root>
  )
}

/** 失敗訊息的錯誤碼 → 翻譯文字（錯誤碼由 adapter 放在 status.error）。 */
function useStatusErrorText(): string | undefined {
  const t = useT()
  const code = useAuiState((s) => {
    const st = s.message.status
    return st?.type === 'incomplete' && st.reason === 'error' ? String(st.error ?? 'internal') : undefined
  })
  return code === undefined ? undefined : errorText(t, new ApiError(0, code, ''))
}

function AiMessage() {
  const t = useT()
  const running = useAuiState((s) => s.message.status?.type === 'running')
  const hasText = useAuiState((s) => s.message.content.some((p) => p.type === 'text' && p.text.length > 0))
  const failure = useStatusErrorText()

  // 達回合上限而自動結束時沒有 AI 回覆：不顯示空泡泡
  if (!hasText && !running && failure === undefined) return null

  return (
    <MessagePrimitive.Root data-role="ai" className="flex flex-col items-start gap-1">
      <span className="text-muted-foreground text-xs">{t('chat.thread.ai')}</span>
      {hasText ? (
        <div className="bg-muted max-w-[85%] rounded-2xl rounded-tl-sm px-4 py-2 text-sm">
          <MessagePrimitive.Parts components={{ Text: PlainText }} />
        </div>
      ) : null}
      {running && !hasText ? (
        <p role="status" className="text-muted-foreground animate-pulse text-sm">
          {t('chat.thread.thinking')}
        </p>
      ) : null}
      {failure !== undefined ? (
        <Alert variant="destructive" role="alert" className="max-w-[85%]">
          <AlertTitle>{failure}</AlertTitle>
          <ActionBarPrimitive.Root className="mt-2">
            <ActionBarPrimitive.Reload asChild>
              <Button variant="outline" size="sm">
                <RefreshCw aria-hidden />
                {t('chat.thread.retry')}
              </Button>
            </ActionBarPrimitive.Reload>
          </ActionBarPrimitive.Root>
        </Alert>
      ) : null}
    </MessagePrimitive.Root>
  )
}

/** 載入歷史時最後一則是學生訊息、AI 還沒回覆（例如上次串流失敗後重新整理）：只重開串流。 */
function PendingReply() {
  const t = useT()
  const aui = useAui()
  const lastUserId = useAuiState((s) => {
    const last = s.thread.messages.at(-1)
    return last?.role === 'user' && !s.thread.isRunning ? last.id : undefined
  })
  if (lastUserId === undefined) return null
  return (
    <Alert role="status">
      <AlertTitle>{t('chat.pending.title')}</AlertTitle>
      <Button variant="outline" size="sm" className="mt-2" onClick={() => aui.thread().startRun({ parentId: lastUserId })}>
        <RefreshCw aria-hidden />
        {t('chat.thread.retry')}
      </Button>
    </Alert>
  )
}

function ChatComposer({ disabled: externalDisabled }: { disabled: boolean }) {
  const t = useT()
  // 最後一則 AI 回覆失敗時鎖住輸入：先按重試，避免在未保存／未回覆的訊息後再疊新訊息
  const lastFailed = useAuiState((s) => {
    const last = s.thread.messages.at(-1)
    return last?.role === 'assistant' && last.status?.type === 'incomplete' && last.status.reason === 'error'
  })
  const disabled = externalDisabled || lastFailed
  return (
    <ComposerPrimitive.Root className="flex flex-wrap items-end gap-2">
      {lastFailed ? (
        <p role="status" className="text-muted-foreground w-full text-sm">
          {t('chat.thread.retryFirst')}
        </p>
      ) : null}
      <ComposerPrimitive.Input
        aria-label={t('chat.thread.inputLabel')}
        placeholder={t('chat.thread.placeholder')}
        maxLength={MAX_LENGTH}
        rows={2}
        disabled={disabled}
        className="border-input bg-background focus-visible:ring-ring/50 max-h-48 min-h-16 w-full min-w-0 resize-none rounded-lg border px-3 py-2 text-base outline-none focus-visible:ring-2 disabled:opacity-50 md:text-sm"
      />
      <ComposerPrimitive.Send asChild>
        {/* 不能直接傳 disabled={false}：會蓋掉 Send 自己依「輸入為空／AI 回覆中」算出的停用狀態 */}
        <Button type="submit" aria-label={t('chat.thread.send')} {...(disabled ? { disabled: true } : {})}>
          <ArrowUp aria-hidden />
          <span className="hidden sm:inline">{t('chat.thread.send')}</span>
        </Button>
      </ComposerPrimitive.Send>
    </ComposerPrimitive.Root>
  )
}
