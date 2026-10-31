/**
 * AI 播放期間監聽學生開口（S-05.5）。組合麥克風、分幀與開口偵測；不碰播放，也不管回合，
 * 只在偵測到開口時呼叫 `onSpeech`，由回合管理（`useVoice`）決定要做什麼。
 *
 * 音訊只在瀏覽器內分析，不上傳也不保存（S-05.4）。任何失敗都只是少了自動打斷：
 * 播放照常，「說話」按鈕仍然可用。
 */
import { BARGE_IN_DETECTOR, BARGE_IN_FRAME_MS, type EchoRisk } from '@/features/chat/voice/config'
import { selectDspStages, startFrameCapture, type DspStage, type FrameCapture } from '@/features/chat/voice/audio/frame-capture'
import {
  MicrophoneError,
  openMicrophone,
  type Microphone,
  type MicrophoneDiagnostics,
  type MicrophoneErrorKind,
} from '@/features/chat/voice/audio/microphone'
import { createSpeechDetector } from '@/features/chat/voice/audio/speech-detector'

/** AI 語音的發聲途徑：`synthesis` 是 speechSynthesis，`element` 是瀏覽器播放的音訊元素。 */
export type PlaybackPath = 'synthesis' | 'element'

/**
 * `getSettings()` 說回音消除開著，不代表消得掉 AI 的聲音：speechSynthesis 不保證進入瀏覽器的回音參考。
 * 只有「瀏覽器自己播放」而且「回報開啟」才視為低風險；其他情況都要更明確的開口證據。
 */
export function assessEchoRisk(path: PlaybackPath, mic: MicrophoneDiagnostics): EchoRisk {
  return path === 'element' && mic.features.echoCancellation.status === 'active' ? 'low' : 'high'
}

/** `muted`：系統在播放時把麥克風靜音，目前已知發生在 iOS，見 S-05.5。 */
export type BargeInFailure = Exclude<MicrophoneErrorKind, 'aborted'> | 'capture' | 'muted'

export type BargeInStatus = {
  state: 'idle' | 'starting' | 'monitoring' | 'failed'
  microphone?: MicrophoneDiagnostics
  echoRisk?: EchoRisk
  failure?: BargeInFailure
}

export type BargeInMonitor = {
  /** 在使用者手勢內呼叫：建立並喚醒 AudioContext，之後才能在沒有手勢時啟動。 */
  unlock: () => void
  /** 開始監聽；AI 開始發聲時呼叫，音量校準才會對到播放。 */
  start: (path: PlaybackPath) => void
  /** 停止監聽並立刻釋放麥克風。 */
  stop: () => void
  /** 連 AudioContext 一起釋放；之後要重新 `unlock`。 */
  close: () => void
}

type Options = {
  onSpeech: () => void
  /** 只在狀態改變時呼叫，不會每幀觸發。 */
  onStatus: (status: BargeInStatus) => void
  stages?: readonly DspStage[]
}

type Run = { ctrl: AbortController; path: PlaybackPath; mic?: Microphone; capture?: FrameCapture; reconnected: boolean }

/**
 * 這些失敗在同一次語音對話內重試也沒用：可能重複跳出權限詢問，
 * 或是每輪都白白佔用音訊工作階段。
 */
const PERMANENT: readonly BargeInFailure[] = ['permission', 'unsupported', 'muted']

export function createBargeInMonitor({ onSpeech, onStatus, stages = [] }: Options): BargeInMonitor {
  let ctx: AudioContext | undefined
  let run: Run | undefined
  let permanentFailure: BargeInFailure | undefined
  let status: BargeInStatus = { state: 'idle' }

  const publish = (next: Partial<BargeInStatus>) => {
    status = { ...status, ...next }
    onStatus(status)
  }

  const fail = (failure: BargeInFailure, cause?: unknown) => {
    console.warn('[voice] barge-in unavailable:', failure, cause ?? '')
    if (PERMANENT.includes(failure)) permanentFailure = failure
    publish({ state: 'failed', failure })
  }

  const release = (r: Run) => {
    r.ctrl.abort()
    r.capture?.stop()
    r.capture = undefined
    r.mic?.close()
    r.mic = undefined
  }

  function ensureContext(): AudioContext | undefined {
    if (!ctx && typeof AudioContext !== 'undefined') ctx = new AudioContext({ latencyHint: 'interactive' })
    return ctx
  }

  const resume = (c: AudioContext) => {
    if (c.state !== 'running') c.resume().catch((e: unknown) => console.warn('[voice] audio context did not resume:', e))
  }

  /** 放棄目前這輪監聽；`r` 已經不是目前這輪時什麼都不做。 */
  function abandon(r: Run, failure: BargeInFailure, cause?: unknown) {
    if (r !== run) return
    stop()
    fail(failure, cause)
  }

  function handleDeviceLost(lost: Run) {
    if (lost !== run) return
    if (lost.reconnected) {
      abandon(lost, 'no-device')
      return
    }
    release(lost)
    // 裝置被拔除時，系統通常會換到預設麥克風：再試一次
    begin(lost.path, true)
  }

  async function connect(r: Run, context: AudioContext) {
    try {
      const mic = await openMicrophone({
        signal: r.ctrl.signal,
        onEnded: () => handleDeviceLost(r),
        onMuted: () => abandon(r, 'muted'),
      })
      r.mic = mic
      const echoRisk = assessEchoRisk(r.path, mic.diagnostics)
      publish({ microphone: mic.diagnostics, echoRisk })
      const detector = createSpeechDetector(BARGE_IN_DETECTOR[echoRisk])
      resume(context)
      let flowing = false
      const capture = await startFrameCapture(context, mic.stream, {
        frameMs: BARGE_IN_FRAME_MS,
        stages: selectDspStages(stages, mic.diagnostics),
        signal: r.ctrl.signal,
        onFrame(frame) {
          if (r !== run) return
          // 第一幀到了才算在聽：AudioContext 在沒有手勢時可能起不來，節點建好不代表有聲音
          if (!flowing) {
            flowing = true
            publish({ state: 'monitoring', failure: undefined })
          }
          if (!detector.push(frame)) return
          // 先放掉麥克風再通知：有些平台的語音辨識不能和 getUserMedia 同時使用
          stop()
          onSpeech()
        },
        onError: (e) => abandon(r, 'capture', e),
      })
      if (r !== run) {
        capture.stop()
        return
      }
      r.capture = capture
    } catch (e) {
      if (!(e instanceof MicrophoneError)) abandon(r, 'capture', e)
      else if (e.kind !== 'aborted') abandon(r, e.kind, e.cause)
    }
  }

  function begin(path: PlaybackPath, reconnected: boolean) {
    if (permanentFailure) return
    const context = ensureContext()
    if (!context) {
      fail('unsupported')
      return
    }
    run = { ctrl: new AbortController(), path, reconnected }
    publish({ state: 'starting', failure: undefined })
    void connect(run, context)
  }

  function stop() {
    const r = run
    run = undefined
    if (r) {
      release(r)
      // 兩次播放之間不留著執行中的 AudioContext，避免佔用音訊工作階段
      ctx?.suspend().catch((e: unknown) => console.warn('[voice] audio context did not suspend:', e))
    }
    if (status.state === 'starting' || status.state === 'monitoring') publish({ state: 'idle' })
  }

  return {
    unlock() {
      const context = ensureContext()
      if (context) resume(context)
      else fail('unsupported')
    },
    start(path) {
      stop()
      begin(path, false)
    },
    stop,
    close() {
      stop()
      ctx?.close().catch((e: unknown) => console.warn('[voice] audio context did not close:', e))
      ctx = undefined
      permanentFailure = undefined
    },
  }
}
