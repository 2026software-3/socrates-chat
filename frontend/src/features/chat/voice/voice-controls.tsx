import { useAui, useAuiState } from '@assistant-ui/react'
import { Mic, RotateCcw, Send, Square, Volume2 } from 'lucide-react'
import { useMemo, type ReactNode } from 'react'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import type { RecognitionSource } from '@/features/chat/voice/recognizer'
import { useVoice, type VoiceHost, type VoiceReply } from '@/features/chat/voice/use-voice'
import { useT } from '@/i18n'

/** 最新一則已完成的 AI 回覆（AI 還在回覆時為 undefined）；用字串讓 selector 結果穩定。 */
function useLatestReply(): VoiceReply | undefined {
  const key = useAuiState((s) => {
    const last = s.thread.messages.at(-1)
    if (s.thread.isRunning || last?.role !== 'assistant' || last.status?.type !== 'complete') return undefined
    const text = last.content.flatMap((p) => (p.type === 'text' ? [p.text] : [])).join('')
    return text ? `${last.id}\u0000${text}` : undefined
  })
  return useMemo(() => {
    if (!key) return undefined
    const i = key.indexOf('\u0000')
    return { id: key.slice(0, i), text: key.slice(i + 1) }
  }, [key])
}

/**
 * 輸入框下方的語音控制：
 * - 「語音輸入」：辨識文字填入輸入框，確認後自己送出（S-05.1）。
 * - 「語音對話」：AI 回覆自動播放、播完自動收音、停頓後倒數送出（S-05.2）。
 * 瀏覽器不支援時自動改走 OpenAI 備援；都不能用時提示改用文字輸入（S-05.3）。
 */
export function VoiceControls({
  language,
  onSource,
  disabled,
  input,
  sendButton,
}: {
  language: string
  onSource: (source: RecognitionSource) => void
  disabled?: boolean
  /** 輸入框（放在膠囊左側） */
  input: ReactNode
  /** 送出按鈕（放在膠囊最右側） */
  sendButton: ReactNode
}) {
  const t = useT()
  const aui = useAui()
  const reply = useLatestReply()

  const host: VoiceHost = {
    language,
    getText: () => aui.composer().getState().text,
    setText: (text) => aui.composer().setText(text),
    send: () => aui.composer().send(),
    onSource,
  }
  const v = useVoice(host, reply)

  const converse = v.mode === 'converse'
  const listening = v.phase === 'listening' || v.phase === 'countdown' || v.phase === 'review'
  const dictating = v.mode === 'dictate' && v.phase === 'listening'

  const status =
    v.phase === 'countdown'
      ? t('voice.status.countdown')
      : v.phase === 'listening'
        ? t('voice.status.listening')
        : v.phase === 'review'
          ? t('voice.status.review')
          : v.phase === 'transcribing'
            ? t('voice.status.transcribing')
            : v.phase === 'waiting'
              ? t('voice.status.waiting')
              : v.phase === 'speaking'
                ? t('voice.status.speaking')
                : converse
                  ? t('voice.status.paused')
                  : ''

  return (
    <div className="w-full min-w-0 space-y-2" data-testid="voice-controls">
      {/* 膠囊輸入列：輸入框在左，語音（即時對話、語音輸入）與送出在右 */}
      <div className="border-input bg-background focus-within:ring-ring/50 flex items-end gap-1 rounded-3xl border py-1.5 pr-1.5 pl-4 shadow-sm focus-within:ring-2">
        {input}
        <Button
          type="button"
          size="lg"
          variant={converse ? 'default' : 'ghost'}
          className="shrink-0 rounded-full px-3"
          aria-label={converse ? t('voice.converse.stop') : t('voice.converse.start')}
          aria-pressed={converse}
          disabled={disabled}
          onClick={v.toggleConverse}
        >
          <Volume2 aria-hidden />
          <span className="text-sm">{converse ? t('voice.converse.shortStop') : t('voice.converse.short')}</span>
        </Button>
        <Button
          type="button"
          size="icon-lg"
          variant={dictating ? 'default' : 'ghost'}
          className="shrink-0 rounded-full"
          aria-label={dictating ? t('voice.dictate.stop') : converse ? t('voice.speak') : t('voice.dictate.start')}
          aria-pressed={dictating}
          disabled={disabled}
          onClick={v.toggleDictate}
        >
          {dictating ? <Square aria-hidden /> : <Mic aria-hidden />}
        </Button>
        {sendButton}
      </div>
      {converse ? (
        <div className="flex items-center gap-2 px-2">
          <Switch id="voice-auto-send" checked={v.autoSend} onCheckedChange={v.setAutoSend} />
          <Label htmlFor="voice-auto-send" className="text-xs sm:text-sm">
            {t('voice.autoSend')}
          </Label>
        </div>
      ) : null}

      {status ? (
        <p role="status" aria-live="polite" className="text-muted-foreground text-sm">
          {status}
        </p>
      ) : null}

      {v.phase === 'countdown' ? (
        <div
          role="progressbar"
          aria-label={t('voice.status.countdown')}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={Math.round(v.countdown * 100)}
          className="bg-muted h-1.5 w-full overflow-hidden rounded-full"
        >
          <div className="bg-primary h-full transition-[width] duration-100" style={{ width: `${v.countdown * 100}%` }} />
        </div>
      ) : null}

      {converse && (listening || v.phase === 'review') ? (
        <div className="flex flex-wrap gap-2">
          <Button type="button" size="sm" onClick={v.sendNow}>
            <Send aria-hidden />
            {t('voice.sendNow')}
          </Button>
          {v.phase === 'countdown' || v.phase === 'review' ? (
            <Button type="button" size="sm" variant="outline" onClick={v.keepListening}>
              {t('voice.notDone')}
            </Button>
          ) : null}
        </div>
      ) : null}

      {converse && v.phase === 'speaking' ? (
        <div className="flex flex-wrap gap-2">
          <Button type="button" size="sm" variant="outline" onClick={v.stopSpeaking}>
            <Square aria-hidden />
            {t('voice.stopSpeaking')}
          </Button>
        </div>
      ) : null}
      {converse && reply && (v.phase === 'idle' || listening) ? (
        <Button type="button" size="sm" variant="ghost" onClick={v.replay}>
          <RotateCcw aria-hidden />
          {t('voice.replay')}
        </Button>
      ) : null}

      {v.notice ? (
        <Alert role="status" variant={v.notice === 'hint' ? 'default' : 'destructive'}>
          <AlertDescription className="flex flex-wrap items-center justify-between gap-2">
            <span>{t(`voice.notice.${v.notice}`)}</span>
            <Button type="button" size="sm" variant="ghost" onClick={v.dismissNotice}>
              {t('voice.dismiss')}
            </Button>
          </AlertDescription>
        </Alert>
      ) : null}
    </div>
  )
}
