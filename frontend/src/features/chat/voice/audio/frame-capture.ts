/**
 * 音訊管線的分幀段（S-05.5）：麥克風 → 可選的 DSP 階段 → AudioWorklet 分幀 → 消費者。
 * 幀處理不經過 React；消費者要自己決定何時才更新畫面。
 */
import { PROCESSING_FEATURES, type MicrophoneDiagnostics, type ProcessingFeature } from '@/features/chat/voice/audio/microphone'

const PROCESSOR_NAME = 'pcm-frame-processor'
const PROCESSOR_URL = `${import.meta.env.BASE_URL}audio/${PROCESSOR_NAME}.js`

export type AudioFrame = { samples: Float32Array; sampleRate: number }

/**
 * 日後接 SpeexDSP WASM 等處理的擴充點：以音訊節點的形式插入，才能跑在音訊執行緒。
 * 回音消除的實作還需要同步的播放參考訊號，這個介面不負責提供，見 S-05.5 文件。
 */
export type DspStage = {
  name: string
  provides: readonly ProcessingFeature[]
  attach: (ctx: AudioContext, input: AudioNode) => Promise<{ output: AudioNode; detach: () => void }>
}

/** 去掉會與瀏覽器原生處理或前面階段重複的項目，避免 AEC／NS／AGC 疊加。 */
export function selectDspStages(stages: readonly DspStage[], native: MicrophoneDiagnostics): DspStage[] {
  const covered = new Set(PROCESSING_FEATURES.filter((f) => native.features[f].status === 'active'))
  return stages.filter((stage) => {
    if (stage.provides.some((f) => covered.has(f))) return false
    stage.provides.forEach((f) => covered.add(f))
    return true
  })
}

const moduleLoads = new WeakMap<AudioContext, Promise<void>>()

function loadProcessor(ctx: AudioContext): Promise<void> {
  const known = moduleLoads.get(ctx)
  if (known) return known
  const load = ctx.audioWorklet.addModule(PROCESSOR_URL)
  moduleLoads.set(ctx, load)
  load.catch(() => moduleLoads.delete(ctx))
  return load
}

export type FrameCapture = {
  /** 拆掉這次建立的節點；可重複呼叫。不負責關閉 `stream` 或 `ctx`。 */
  stop: () => void
}

type CaptureOptions = {
  frameMs: number
  onFrame: (frame: AudioFrame) => void
  /** AudioWorklet 執行中當掉。 */
  onError: (error: Error) => void
  stages?: readonly DspStage[]
  signal?: AbortSignal
}

export async function startFrameCapture(ctx: AudioContext, stream: MediaStream, opts: CaptureOptions): Promise<FrameCapture> {
  if (!ctx.audioWorklet || typeof AudioWorkletNode === 'undefined') throw new Error('AudioWorklet is not available')
  await loadProcessor(ctx)
  opts.signal?.throwIfAborted()

  const source = ctx.createMediaStreamSource(stream)
  const undo: (() => void)[] = [() => source.disconnect()]
  const teardown = () => undo.splice(0).reverse().forEach((fn) => fn())

  try {
    let tail: AudioNode = source
    for (const stage of opts.stages ?? []) {
      const { output, detach } = await stage.attach(ctx, tail)
      undo.push(detach)
      opts.signal?.throwIfAborted()
      tail = output
    }

    const { sampleRate } = ctx
    const frameSamples = Math.round((sampleRate * opts.frameMs) / 1000)
    const node = new AudioWorkletNode(ctx, PROCESSOR_NAME, {
      numberOfInputs: 1,
      numberOfOutputs: 1,
      channelCount: 1,
      channelCountMode: 'explicit',
      processorOptions: { frameSamples },
    })
    node.port.onmessage = (e: MessageEvent<ArrayBuffer>) => opts.onFrame({ samples: new Float32Array(e.data), sampleRate })
    node.onprocessorerror = () => opts.onError(new Error('audio worklet processor failed'))
    undo.push(() => {
      node.port.onmessage = null
      node.onprocessorerror = null
      node.disconnect()
      node.port.close()
    })
    tail.connect(node)
    // 接到 destination 才保證各瀏覽器都會驅動它；process 不寫輸出，所以是靜音的
    node.connect(ctx.destination)
    return { stop: teardown }
  } catch (e) {
    teardown()
    throw e
  }
}
