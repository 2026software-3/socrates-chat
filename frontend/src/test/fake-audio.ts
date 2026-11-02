import { vi } from 'vitest'

/** 測試用的假瀏覽器音訊 API：只模擬語音前處理會碰到的部分。 */

type Listener = () => void

export class FakeTrack {
  readyState: 'live' | 'ended' = 'live'
  stop = vi.fn(() => {
    this.readyState = 'ended'
  })
  listeners = new Set<Listener>()
  muteListeners = new Set<Listener>()
  settings: MediaTrackSettings
  constructor(settings: MediaTrackSettings) {
    this.settings = settings
  }
  getSettings() {
    return this.settings
  }
  addEventListener(type: string, fn: Listener) {
    if (type === 'ended') this.listeners.add(fn)
    if (type === 'mute') this.muteListeners.add(fn)
  }
  removeEventListener(type: string, fn: Listener) {
    if (type === 'ended') this.listeners.delete(fn)
    if (type === 'mute') this.muteListeners.delete(fn)
  }
  /** 系統把麥克風靜音：軌道還是 live，但收不到聲音（例如 iOS 播放語音時）。 */
  muteBySystem() {
    this.muteListeners.forEach((fn) => fn())
  }
  /** 裝置被拔除：瀏覽器把軌道結束並發出 `ended`。 */
  unplug() {
    this.readyState = 'ended'
    this.listeners.forEach((fn) => fn())
  }
}

export function fakeStream(track: FakeTrack) {
  return { getTracks: () => [track], getAudioTracks: () => [track] } as unknown as MediaStream
}

export type FakeMediaOptions = {
  supported?: MediaTrackSupportedConstraints
  settings?: MediaTrackSettings
  getUserMedia?: (c: MediaStreamConstraints) => Promise<MediaStream>
}

/** 三項處理都支援也都套用的瀏覽器。 */
export const ALL_SUPPORTED = { echoCancellation: true, noiseSuppression: true, autoGainControl: true }
export const ALL_APPLIED = { echoCancellation: true, noiseSuppression: true, autoGainControl: true }

export function installMediaDevices(opts: FakeMediaOptions = {}) {
  const tracks: FakeTrack[] = []
  const getUserMedia = vi.fn(
    opts.getUserMedia ??
      (() => {
        const track = new FakeTrack(opts.settings ?? ALL_APPLIED)
        tracks.push(track)
        return Promise.resolve(fakeStream(track))
      }),
  )
  Object.defineProperty(navigator, 'mediaDevices', {
    configurable: true,
    value: { getSupportedConstraints: () => opts.supported ?? ALL_SUPPORTED, getUserMedia },
  })
  return { tracks, getUserMedia }
}

export function removeMediaDevices() {
  Object.defineProperty(navigator, 'mediaDevices', { configurable: true, value: undefined })
}

export class FakeWorkletNode {
  static instances: FakeWorkletNode[] = []
  port = {
    onmessage: null as ((e: { data: ArrayBuffer }) => void) | null,
    close: vi.fn(),
    postMessage: vi.fn(),
  }
  onprocessorerror: (() => void) | null = null
  connect = vi.fn()
  disconnect = vi.fn()
  options: AudioWorkletNodeOptions
  constructor(_ctx: unknown, _name: string, options: AudioWorkletNodeOptions) {
    this.options = options
    FakeWorkletNode.instances.push(this)
  }
  emit(samples: Float32Array) {
    const copy = samples.slice()
    this.port.onmessage?.({ data: copy.buffer })
  }
  static get last() {
    return FakeWorkletNode.instances[FakeWorkletNode.instances.length - 1]
  }
}

export class FakeAudioContext {
  static instances: FakeAudioContext[] = []
  sampleRate = 48_000
  state: 'suspended' | 'running' | 'closed' = 'suspended'
  destination = { kind: 'destination' }
  sources: { connect: ReturnType<typeof vi.fn>; disconnect: ReturnType<typeof vi.fn> }[] = []
  audioWorklet = { addModule: vi.fn(() => Promise.resolve()) }
  resume = vi.fn(() => {
    this.state = 'running'
    return Promise.resolve()
  })
  suspend = vi.fn(() => {
    this.state = 'suspended'
    return Promise.resolve()
  })
  close = vi.fn(() => {
    this.state = 'closed'
    return Promise.resolve()
  })
  constructor() {
    FakeAudioContext.instances.push(this)
  }
  createMediaStreamSource() {
    const src = { connect: vi.fn(), disconnect: vi.fn() }
    this.sources.push(src)
    return src
  }
  static get last() {
    return FakeAudioContext.instances[FakeAudioContext.instances.length - 1]
  }
}

export function installAudioGraph() {
  FakeAudioContext.instances = []
  FakeWorkletNode.instances = []
  vi.stubGlobal('AudioContext', FakeAudioContext)
  vi.stubGlobal('AudioWorkletNode', FakeWorkletNode)
}

// ---- 合成訊號（不使用任何真實語音）----

const rmsToAmp = (db: number) => 10 ** (db / 20)

/** 有基頻與諧波的「濁音」：週期性高，像人聲或 TTS。相位連續。 */
export function voicedSource(sampleRate: number, db: number, f0 = 140) {
  let n = 0
  return (ms: number) => {
    const out = new Float32Array(Math.round((sampleRate * ms) / 1000))
    const amp = rmsToAmp(db) * 1.25
    for (let i = 0; i < out.length; i++, n++) {
      const t = n / sampleRate
      let s = 0
      for (let k = 1; k <= 5; k++) s += Math.sin(2 * Math.PI * k * f0 * t) / k
      out[i] = amp * s
    }
    return out
  }
}

/** 白噪音（固定種子）：音量可以很大，但沒有週期性，像鍵盤或風扇。 */
export function noiseSource(sampleRate: number, db: number, seed = 1) {
  let x = seed
  return (ms: number) => {
    const out = new Float32Array(Math.round((sampleRate * ms) / 1000))
    const amp = rmsToAmp(db) * Math.sqrt(3)
    for (let i = 0; i < out.length; i++) {
      x = (x * 1664525 + 1013904223) % 4294967296
      out[i] = amp * (x / 2147483648 - 1)
    }
    return out
  }
}

/** 把一段時間切成固定長度的幀，逐幀交給 `push`。 */
export function feed(source: (ms: number) => Float32Array, totalMs: number, push: (samples: Float32Array) => void, frameMs = 20) {
  for (let t = 0; t < totalMs; t += frameMs) push(source(frameMs))
}
