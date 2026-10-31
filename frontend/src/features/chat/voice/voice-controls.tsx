import { useAui, useAuiState } from '@assistant-ui/react'
import { Mic, RotateCcw, Send, Square, Volume2 } from 'lucide-react'
import { useMemo, type ReactNode } from 'react'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import type { BargeInStatus } from '@/features/chat/voice/audio/barge-in'
import { PROCESSING_FEATURES, type FeatureStatus, type ProcessingFeature } from '@/features/chat/voice/audio/microphone'
import type { RecognitionSource } from '@/features/chat/voice/recognizer'
import { useVoice, type VoiceHost, type VoicePhase, type VoiceReply } from '@/features/chat/voice/use-voice'
import { useT, type MessageKey } from '@/i18n'

/** 各階段的狀態文字；`idle` 另外處理（對話模式顯示「暫停」）。 */
const PHASE_STATUS: Record<Exclude<VoicePhase, 'idle'>, MessageKey> = {
  countdown: 'voice.status.countdown',
  listening: 'voice.status.listening',
  review: 'voice.status.review',
  transcribing: 'voice.status.transcribing',
  waiting: 'voice.status.waiting',
  speaking: 'voice.status.speaking',
}

/** 最新一則 AI 回覆，包含還在生成的內容；用字串讓 selector 結果穩定。 */
function useLatestReply(): VoiceReply | undefined {
  const reply = useAuiState((s) => {
    const last = s.thread.messages.at(-1)
    if (last?.role !== 'assistant') return undefined
    const text = last.content.flatMap((p) => (p.type === 'text' ? [p.text] : [])).join('')
    const status: VoiceReply['status'] = s.thread.isRunning
      ? 'running'
      : last.status?.type === 'incomplete'
        ? 'failed'
        : 'complete'
    return JSON.stringify({ id: last.id, text, status } satisfies VoiceReply)
  })
  return useMemo(() => (reply ? (JSON.parse(reply) as VoiceReply) : undefined), [reply])
}

const FEATURE_LABEL: Record<ProcessingFeature, string> = {
  echoCancellation: 'AEC',
  noiseSuppression: 'NS',
  autoGainControl: 'AGC',
}

const FEATURE_STATUS: Record<FeatureStatus, MessageKey> = {
  active: 'voice.audio.active',
  inactive: 'voice.audio.inactive',
  unreported: 'voice.audio.unreported',
  unsupported: 'voice.audio.unsupported',
}

const MONITOR_STATE: Record<BargeInStatus['state'], MessageKey> = {
  idle: 'voice.audio.state.idle',
  starting: 'voice.audio.state.starting',
  monitoring: 'voice.audio.state.monitoring',
  failed: 'voice.audio.state.failed',
}

/** 學生回報語音問題時看這裡：瀏覽器回報的狀態，不代表實際效果（S-05.5）。 */
function AudioDiagnostics({ status }: { status: BargeInStatus }) {
  const t = useT()
  const { microphone, echoRisk, failure } = status
  return (
    <details className="text-muted-foreground px-2 text-xs" data-testid="voice-audio-diagnostics">
      <summary className="cursor-pointer">{t('voice.audio.title')}</summary>
      <ul className="mt-1 space-y-0.5">
        <li>
          {t('voice.audio.monitor')}: {t(MONITOR_STATE[status.state])}
          {failure ? ` (${failure})` : ''}
        </li>
        {microphone
          ? PROCESSING_FEATURES.map((f) => (
              <li key={f}>
                {FEATURE_LABEL[f]}: {t(FEATURE_STATUS[microphone.features[f].status])}
              </li>
            ))
          : null}
        {echoRisk === 'high' ? <li>{t('voice.audio.echoHigh')}</li> : null}
      </ul>
    </details>
  )
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

  const status = v.phase !== 'idle' ? t(PHASE_STATUS[v.phase]) : converse ? t('voice.status.paused') : ''

  return (
    <div className="w-full min-w-0 space-y-2" data-testid="voice-controls">
      {/* 膠囊輸入列：輸入框在左，語音（即時對話、語音輸入）與送出在右 */}
      <div className="bg-surface focus-within:ring-ring/50 flex items-end gap-1 rounded-3xl border py-1.5 pr-1.5 pl-5 focus-within:ring-2">
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
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2 px-2">
          <div className="flex items-center gap-2">
            <Switch id="voice-auto-send" checked={v.autoSend} onCheckedChange={v.setAutoSend} />
            <Label htmlFor="voice-auto-send" className="text-xs sm:text-sm">
              {t('voice.autoSend')}
            </Label>
          </div>
          <div className="flex items-center gap-2">
            <Switch id="voice-barge-in" checked={v.bargeIn} onCheckedChange={v.setBargeIn} />
            <Label htmlFor="voice-barge-in" className="text-xs sm:text-sm">
              {t('voice.bargeIn')}
            </Label>
          </div>
        </div>
      ) : null}
      {converse && v.bargeIn ? <AudioDiagnostics status={v.audioStatus} /> : null}

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

      {converse && listening ? (
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
      {converse && reply?.status === 'complete' && reply.text && (v.phase === 'idle' || listening) ? (
        <Button type="button" size="sm" variant="ghost" onClick={v.replay}>
          <RotateCcw aria-hidden />
          {t('voice.replay')}
        </Button>
      ) : null}

      {v.notice ? (
        <Alert role="status" variant={v.notice === 'hint' || v.notice === 'bargeIn' ? 'default' : 'destructive'}>
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
