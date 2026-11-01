/**
 * 伺服器端語音對話的一場連線：麥克風 → 分幀 → 16 kHz PCM → WebSocket，以及 AI 語音的播放。
 *
 * 開口、斷句與取消回覆都由伺服器的 cascade 決定，這裡只照事件做事，不碰畫面與對話串。
 * 音訊只轉送，不保存（S-05.4）；這裡不記錄逐字稿、回覆文字或音訊內容。
 */
import { startFrameCapture, type FrameCapture } from '@/features/chat/voice/audio/frame-capture'
import { MicrophoneError, openMicrophone, type Microphone, type MicrophoneDiagnostics } from '@/features/chat/voice/audio/microphone'
import { createDownsampler, decodePcm16, encodePcm16 } from '@/features/chat/voice/realtime/pcm'
import { createPcmPlayer, type PcmPlayer } from '@/features/chat/voice/realtime/pcm-player'

/** cascade 收 16 kHz、送 24 kHz，都是 16 位元單聲道。 */
const INPUT_RATE = 16_000
const OUTPUT_RATE = 24_000
const FRAME_MS = 100
/** 瀏覽器讀不到 WebSocket 升級失敗的原因，也可能連上後端但 cascade 沒回應。 */
const CONNECT_TIMEOUT_MS = 10_000

export type RealtimePhase = 'connecting' | 'listening' | 'transcribing' | 'waiting' | 'speaking'

/**
 * - `unreachable`：連不上（服務沒開、另一個分頁在用、權限不符），可改用其他語音方式
 * - `lost`：對話中斷線
 * - `replyFailed`：這一輪沒有存成回覆，要先重試才能繼續
 * - `permission`／`device`／`unavailable`：麥克風或瀏覽器問題
 */
export type RealtimeCloseReason = 'unreachable' | 'lost' | 'replyFailed' | 'permission' | 'device' | 'unavailable'

export type RealtimeCallbacks = {
  onPhase: (phase: RealtimePhase) => void
  onMicrophone: (diagnostics: MicrophoneDiagnostics) => void
  onTranscript: (text: string) => void
  onReplyDelta: (text: string) => void
  onReplyEnd: () => void
  /** 伺服器說這一輪失敗；在 `onReplyEnd` 之前呼叫，接著以 `replyFailed` 關閉。 */
  onReplyFailed: () => void
  /** 這一輪結束且已唸完（或被打斷）：可以同步對話狀態。 */
  onSettled: () => void
  /** 非 `close()` 造成的結束；之後不會再有任何回呼。 */
  onClosed: (reason: RealtimeCloseReason) => void
}

export type RealtimeSession = {
  /** 停止 AI 說話並把回合交給學生。 */
  interrupt: () => void
  /** 關閉時，回覆期間上傳靜音：AI 的聲音不會打斷自己。 */
  setInterruptible: (on: boolean) => void
  /** 釋放所有資源；可重複呼叫，不觸發 `onClosed`。 */
  close: () => void
}

type Reply = {
  id: string
  item?: string
  interrupted: boolean
  serverDone: boolean
  textEnded: boolean
}

type ServerEvent = {
  type: string
  transcript?: string
  delta?: string
  response_id?: string
  item_id?: string
  response?: { id?: string }
  sglang?: { fatal?: boolean }
  error?: { code?: string }
}

export function realtimeUrl(conversationId: string, location: Pick<Location, 'protocol' | 'host'> = window.location): string {
  const scheme = location.protocol === 'https:' ? 'wss:' : 'ws:'
  return `${scheme}//${location.host}/api/conversations/${conversationId}/voice`
}

const microphoneReason = (e: unknown): RealtimeCloseReason => {
  switch (e instanceof MicrophoneError ? e.kind : 'failed') {
    case 'permission':
      return 'permission'
    case 'no-device':
    case 'busy':
      return 'device'
    default:
      return 'unavailable'
  }
}

/** 伺服器給的錯誤碼要進 console：只放行短的英數底線字串，其餘（換行、控制字元、過長）一律不照印。 */
const loggableCode = (code: unknown): string => (typeof code === 'string' && /^[\w.-]{1,64}$/.test(code) ? code : 'unknown')

/** 必須在使用者手勢內呼叫：AudioContext 要在這裡喚醒，之後才能沒有手勢地播放。 */
export function createRealtimeSession(conversationId: string, cb: RealtimeCallbacks, interruptible: boolean): RealtimeSession {
  const ctrl = new AbortController()
  const ctx = new AudioContext({ latencyHint: 'interactive' })
  const player: PcmPlayer = createPcmPlayer(ctx, OUTPUT_RATE, () => settle())
  let mic: Microphone | undefined
  let capture: FrameCapture | undefined
  let socket: WebSocket | undefined
  let watchdog: number | undefined
  let closed = false
  let answered = false
  let seq = 0
  let reply: Reply | undefined
  let lastPhase: RealtimePhase | undefined
  let allowInterrupt = interruptible

  ctx.resume().catch((e: unknown) => console.warn('[voice] audio context did not resume:', e))

  const phase = (next: RealtimePhase) => {
    if (next === lastPhase) return
    lastPhase = next
    cb.onPhase(next)
  }

  const release = () => {
    closed = true
    ctrl.abort()
    window.clearTimeout(watchdog)
    capture?.stop()
    capture = undefined
    mic?.close()
    mic = undefined
    player.clear()
    if (socket) {
      socket.onopen = null
      socket.onmessage = null
      socket.onerror = null
      socket.onclose = null
      socket.close()
      socket = undefined
    }
    ctx.close().catch((e: unknown) => console.warn('[voice] audio context did not close:', e))
  }

  const endText = (r: Reply) => {
    if (r.textEnded) return
    r.textEnded = true
    cb.onReplyEnd()
  }

  const fail = (reason: RealtimeCloseReason) => {
    if (closed) return
    if (reply) endText(reply)
    release()
    cb.onClosed(reason)
  }

  const send = (event: Record<string, unknown>) => {
    if (socket?.readyState === WebSocket.OPEN) {
      socket.send(JSON.stringify({ ...event, event_id: crypto.randomUUID().replaceAll('-', '') }))
    }
  }

  function settle() {
    if (closed || !reply || !reply.serverDone || player.busy()) return
    reply = undefined
    // 同一個播放器跨回合使用，打斷時回報的時間只能算當前這則回覆
    player.clear()
    phase('listening')
    cb.onSettled()
  }

  /** 清掉本機還沒唸的部分，並告訴伺服器學生實際聽到哪裡；回到聆聽。 */
  const cutOff = (r: Reply) => {
    r.interrupted = true
    if (r.item) send({ type: 'conversation.item.truncate', item_id: r.item, content_index: 0, audio_end_ms: player.playedMs() })
    player.clear()
    endText(r)
    phase('listening')
    // 伺服器已先結束這則回覆時，不會再有 `response.done`
    settle()
  }

  const onEvent = (event: ServerEvent) => {
    if (!answered) {
      answered = true
      window.clearTimeout(watchdog)
      phase('listening')
    }
    switch (event.type) {
      case 'input_audio_buffer.speech_started':
        if (reply && !reply.interrupted) cutOff(reply)
        break
      case 'input_audio_buffer.speech_stopped':
        if (!reply || reply.interrupted) phase('transcribing')
        break
      case 'conversation.item.input_audio_transcription.completed': {
        const text = event.transcript?.trim()
        if (text) cb.onTranscript(text)
        break
      }
      case 'response.created':
        reply = { id: event.response?.id ?? '', interrupted: false, serverDone: false, textEnded: false }
        phase('waiting')
        break
      case 'response.output_text.delta':
        if (reply && !reply.interrupted && event.response_id === reply.id && event.delta) cb.onReplyDelta(event.delta)
        break
      case 'response.output_audio.delta':
        if (reply && !reply.interrupted && event.response_id === reply.id && event.delta) {
          reply.item = event.item_id
          player.push(decodePcm16(event.delta))
          phase('speaking')
        }
        break
      case 'response.done':
        if (reply && event.response?.id === reply.id) {
          reply.serverDone = true
          endText(reply)
          settle()
        }
        break
      case 'socrates.turn.failed':
        cb.onReplyFailed()
        fail('replyFailed')
        break
      case 'error':
        console.warn('[voice] realtime error:', loggableCode(event.error?.code))
        if (event.sglang?.fatal) fail('lost')
        break
      default:
    }
  }

  const startCapture = async (microphone: Microphone) => {
    const down = createDownsampler(ctx.sampleRate, INPUT_RATE)
    const started = await startFrameCapture(ctx, microphone.stream, {
      frameMs: FRAME_MS,
      signal: ctrl.signal,
      onFrame({ samples }) {
        if (closed) return
        const pcm = down(samples)
        if (pcm.length === 0) return
        // 伺服器的開口偵測只看收到的音訊，停送會卡住這一輪；序號也必須連續，所以靜音照送
        if (!allowInterrupt && reply && !reply.interrupted) pcm.fill(0)
        send({ type: 'input_audio_buffer.append', audio: encodePcm16(pcm), sglang: { seq } })
        seq += 1
      },
      onError: () => fail('unavailable'),
    })
    if (closed) started.stop()
    else capture = started
  }

  const connect = (microphone: Microphone) => {
    const ws = new WebSocket(realtimeUrl(conversationId))
    socket = ws
    watchdog = window.setTimeout(() => fail('unreachable'), CONNECT_TIMEOUT_MS)
    ws.onopen = () => {
      startCapture(microphone).catch((e: unknown) => {
        if (closed) return
        console.warn('[voice] capture failed:', e)
        fail('unavailable')
      })
    }
    ws.onmessage = (e: MessageEvent<string>) => onEvent(JSON.parse(e.data) as ServerEvent)
    const dropped = () => fail(answered ? 'lost' : 'unreachable')
    ws.onerror = dropped
    ws.onclose = dropped
  }

  phase('connecting')
  openMicrophone({
    signal: ctrl.signal,
    onEnded: () => fail('device'),
    onMuted: () => fail('device'),
  }).then(
    (microphone) => {
      if (closed) {
        microphone.close()
        return
      }
      mic = microphone
      cb.onMicrophone(microphone.diagnostics)
      connect(microphone)
    },
    (e: unknown) => fail(microphoneReason(e)),
  )

  return {
    interrupt() {
      if (closed || !reply || reply.interrupted) return
      send({ type: 'response.cancel' })
      cutOff(reply)
    },
    setInterruptible(on) {
      allowInterrupt = on
    },
    close() {
      if (!closed) release()
    },
  }
}
