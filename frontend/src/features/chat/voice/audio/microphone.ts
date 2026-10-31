/**
 * 開麥克風並回報前處理的實際狀態（S-05.5）。
 *
 * 只看瀏覽器執行時回報的能力，不看 User-Agent。這裡的 `active` 只代表瀏覽器說它開著，
 * 不代表實際有效：回音消除消不消得掉 AI 的聲音，還要看音訊怎麼播（見 `assessEchoRisk`）。
 */

export const PROCESSING_FEATURES = ['echoCancellation', 'noiseSuppression', 'autoGainControl'] as const
export type ProcessingFeature = (typeof PROCESSING_FEATURES)[number]

/**
 * - `active`：`getSettings()` 回報開啟
 * - `inactive`：`getSettings()` 回報關閉（可能被裝置或瀏覽器拒絕）
 * - `unreported`：瀏覽器有列出，但 `getSettings()` 沒回報：不能假設有開
 * - `unsupported`：`getSupportedConstraints()` 沒列出，也沒有要求
 */
export type FeatureStatus = 'active' | 'inactive' | 'unreported' | 'unsupported'

export type FeatureReport = {
  supported: boolean
  requested: boolean
  applied: boolean | undefined
  status: FeatureStatus
}

/** 疑難排解用；刻意不含 deviceId、groupId 與裝置名稱。 */
export type MicrophoneDiagnostics = {
  features: Record<ProcessingFeature, FeatureReport>
  sampleRate?: number
  channelCount?: number
  latency?: number
}

export function statusOf(d: MicrophoneDiagnostics): Record<ProcessingFeature, FeatureStatus> {
  return {
    echoCancellation: d.features.echoCancellation.status,
    noiseSuppression: d.features.noiseSuppression.status,
    autoGainControl: d.features.autoGainControl.status,
  }
}

/**
 * - `permission`：使用者或政策拒絕
 * - `no-device`：沒有麥克風
 * - `busy`：裝置被占用或硬體錯誤
 * - `unsupported`：瀏覽器沒有這個 API（例如非安全來源）
 * - `aborted`：呼叫端已放棄
 * - `failed`：其他失敗
 */
export type MicrophoneErrorKind = 'permission' | 'no-device' | 'busy' | 'unsupported' | 'aborted' | 'failed'

export class MicrophoneError extends Error {
  readonly kind: MicrophoneErrorKind
  constructor(kind: MicrophoneErrorKind, cause?: unknown) {
    super(`microphone: ${kind}`, { cause })
    this.name = 'MicrophoneError'
    this.kind = kind
  }
}

function classify(e: unknown): MicrophoneError {
  switch ((e as { name?: string } | undefined)?.name) {
    case 'NotAllowedError':
    case 'SecurityError':
      return new MicrophoneError('permission', e)
    case 'NotFoundError':
      return new MicrophoneError('no-device', e)
    case 'NotReadableError':
    case 'AbortError':
      return new MicrophoneError('busy', e)
    default:
      return new MicrophoneError('failed', e)
  }
}

function supportedFeatures(): Record<ProcessingFeature, boolean> {
  const listed: MediaTrackSupportedConstraints = navigator.mediaDevices.getSupportedConstraints?.() ?? {}
  return {
    echoCancellation: listed.echoCancellation === true,
    noiseSuppression: listed.noiseSuppression === true,
    autoGainControl: listed.autoGainControl === true,
  }
}

/** 只要求瀏覽器有列出的項目。布林值是 `ideal`，瀏覽器做不到也不會造成 OverconstrainedError。 */
function constraintsFor(supported: Record<ProcessingFeature, boolean>): MediaTrackConstraints | true {
  const wanted = PROCESSING_FEATURES.filter((f) => supported[f])
  return wanted.length > 0 ? Object.fromEntries(wanted.map((f) => [f, true])) : true
}

function reportFeature(feature: ProcessingFeature, supported: boolean, requested: boolean, settings: MediaTrackSettings): FeatureReport {
  // 規格新增的模式字串也可能出現；`remote-only` 不處理本機播放，對我們等於沒開
  const raw = settings[feature] as boolean | string | undefined
  if (raw === undefined) return { supported, requested, applied: undefined, status: supported ? 'unreported' : 'unsupported' }
  const applied = raw !== false && raw !== 'remote-only'
  return { supported, requested, applied, status: applied ? 'active' : 'inactive' }
}

function diagnose(
  supported: Record<ProcessingFeature, boolean>,
  constraints: MediaTrackConstraints | true,
  settings: MediaTrackSettings,
): MicrophoneDiagnostics {
  const requested = (f: ProcessingFeature) => constraints !== true && f in constraints
  return {
    features: {
      echoCancellation: reportFeature('echoCancellation', supported.echoCancellation, requested('echoCancellation'), settings),
      noiseSuppression: reportFeature('noiseSuppression', supported.noiseSuppression, requested('noiseSuppression'), settings),
      autoGainControl: reportFeature('autoGainControl', supported.autoGainControl, requested('autoGainControl'), settings),
    },
    sampleRate: settings.sampleRate,
    channelCount: settings.channelCount,
    latency: (settings as { latency?: number }).latency,
  }
}

export type Microphone = {
  stream: MediaStream
  diagnostics: MicrophoneDiagnostics
  /** 停止所有軌道；可重複呼叫。呼叫後不會再收到 `onEnded`、`onMuted`。 */
  close: () => void
}

type OpenOptions = {
  signal?: AbortSignal
  /** 非我方關閉造成的軌道結束，例如裝置被拔除。 */
  onEnded?: () => void
  /** 軌道被系統靜音：`readyState` 仍是 `live`，但收不到聲音。 */
  onMuted?: () => void
}

/** 呼叫端擁有回傳的 `Microphone`，用完必須 `close`；被放棄時這裡會自己釋放。 */
export async function openMicrophone({ signal, onEnded, onMuted }: OpenOptions = {}): Promise<Microphone> {
  if (signal?.aborted) throw new MicrophoneError('aborted')
  if (!navigator.mediaDevices?.getUserMedia) throw new MicrophoneError('unsupported')

  const supported = supportedFeatures()
  const constraints = constraintsFor(supported)
  let stream: MediaStream
  try {
    stream = await navigator.mediaDevices.getUserMedia({ audio: constraints })
  } catch (e) {
    throw classify(e)
  }

  const stopTracks = () => stream.getTracks().forEach((t) => t.stop())
  if (signal?.aborted) {
    stopTracks()
    throw new MicrophoneError('aborted')
  }
  const track = stream.getAudioTracks()[0]
  if (!track) {
    stopTracks()
    throw new MicrophoneError('failed')
  }

  const ended = () => onEnded?.()
  const muted = () => onMuted?.()
  track.addEventListener('ended', ended)
  track.addEventListener('mute', muted)
  let closed = false
  return {
    stream,
    diagnostics: diagnose(supported, constraints, track.getSettings()),
    close() {
      if (closed) return
      closed = true
      track.removeEventListener('ended', ended)
      track.removeEventListener('mute', muted)
      stopTracks()
    },
  }
}
