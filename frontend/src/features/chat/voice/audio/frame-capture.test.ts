/// <reference types="node" />

import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { selectDspStages, startFrameCapture, type AudioFrame, type DspStage } from '@/features/chat/voice/audio/frame-capture'
import type { MicrophoneDiagnostics } from '@/features/chat/voice/audio/microphone'
import { FakeAudioContext, FakeTrack, FakeWorkletNode, fakeStream, installAudioGraph } from '@/test/fake-audio'

const stream = () => fakeStream(new FakeTrack({}))
const context = () => new FakeAudioContext() as unknown as AudioContext

beforeEach(installAudioGraph)
afterEach(() => vi.unstubAllGlobals())

describe('startFrameCapture', () => {
  it('routes the microphone through the worklet and hands frames to the consumer', async () => {
    const frames: AudioFrame[] = []
    const ctx = context()
    await startFrameCapture(ctx, stream(), { frameMs: 20, onFrame: (f) => frames.push(f), onError: () => {} })
    const node = FakeWorkletNode.last
    expect(node.options.processorOptions).toEqual({ frameSamples: 960 })
    expect(node.options.channelCount).toBe(1)
    expect(FakeAudioContext.last.sources[0].connect).toHaveBeenCalledWith(node)
    node.emit(new Float32Array(960).fill(0.5))
    expect(frames).toHaveLength(1)
    expect(frames[0].sampleRate).toBe(48_000)
    expect(frames[0].samples[0]).toBe(0.5)
  })

  it('loads the worklet module once per context', async () => {
    const ctx = context()
    const one = await startFrameCapture(ctx, stream(), { frameMs: 20, onFrame: () => {}, onError: () => {} })
    one.stop()
    await startFrameCapture(ctx, stream(), { frameMs: 20, onFrame: () => {}, onError: () => {} })
    expect(FakeAudioContext.last.audioWorklet.addModule).toHaveBeenCalledTimes(1)
  })

  it('stop disconnects everything and delivers no more frames', async () => {
    const onFrame = vi.fn()
    const capture = await startFrameCapture(context(), stream(), { frameMs: 20, onFrame, onError: () => {} })
    const node = FakeWorkletNode.last
    capture.stop()
    capture.stop()
    expect(node.disconnect).toHaveBeenCalledTimes(1)
    expect(FakeAudioContext.last.sources[0].disconnect).toHaveBeenCalledTimes(1)
    expect(node.port.close).toHaveBeenCalledTimes(1)
    expect(node.port.onmessage).toBeNull()
    node.emit(new Float32Array(960))
    expect(onFrame).not.toHaveBeenCalled()
  })

  it('builds nothing if aborted while the module was loading', async () => {
    const ctrl = new AbortController()
    const pending = startFrameCapture(context(), stream(), { frameMs: 20, onFrame: () => {}, onError: () => {}, signal: ctrl.signal })
    ctrl.abort()
    await expect(pending).rejects.toThrow()
    expect(FakeWorkletNode.instances).toHaveLength(0)
  })

  it('rejects when the browser has no AudioWorklet instead of using ScriptProcessorNode', async () => {
    const ctx = context()
    Object.assign(ctx, { audioWorklet: undefined })
    await expect(startFrameCapture(ctx, stream(), { frameMs: 20, onFrame: () => {}, onError: () => {} })).rejects.toThrow()
  })

  it('reports a crashed processor', async () => {
    const onError = vi.fn()
    await startFrameCapture(context(), stream(), { frameMs: 20, onFrame: () => {}, onError })
    FakeWorkletNode.last.onprocessorerror?.()
    expect(onError).toHaveBeenCalledTimes(1)
  })

  it('inserts DSP stages between the microphone and the framer, and detaches them', async () => {
    const output = { connect: vi.fn() } as unknown as AudioNode
    const detach = vi.fn()
    const attach = vi.fn(() => Promise.resolve({ output, detach }))
    const stage: DspStage = { name: 'test', provides: ['noiseSuppression'], attach }
    const capture = await startFrameCapture(context(), stream(), { frameMs: 20, onFrame: () => {}, onError: () => {}, stages: [stage] })
    expect(attach).toHaveBeenCalledTimes(1)
    expect(output.connect).toHaveBeenCalledWith(FakeWorkletNode.last)
    capture.stop()
    expect(detach).toHaveBeenCalledTimes(1)
  })

  it('detaches stages already attached when a later step fails', async () => {
    const detach = vi.fn()
    const output = { connect: vi.fn() } as unknown as AudioNode
    const good: DspStage = { name: 'a', provides: [], attach: () => Promise.resolve({ output, detach }) }
    const bad: DspStage = { name: 'b', provides: [], attach: () => Promise.reject(new Error('wasm failed')) }
    await expect(startFrameCapture(context(), stream(), { frameMs: 20, onFrame: () => {}, onError: () => {}, stages: [good, bad] })).rejects.toThrow('wasm failed')
    expect(detach).toHaveBeenCalledTimes(1)
    expect(FakeAudioContext.last.sources[0].disconnect).toHaveBeenCalledTimes(1)
  })
})

describe('selectDspStages', () => {
  const stage = (name: string, provides: DspStage['provides']): DspStage => ({
    name,
    provides,
    attach: () => Promise.reject(new Error('unused')),
  })
  const diagnostics = (echo: 'active' | 'unsupported'): MicrophoneDiagnostics => ({
    features: {
      echoCancellation: { supported: echo === 'active', requested: echo === 'active', applied: echo === 'active' ? true : undefined, status: echo },
      noiseSuppression: { supported: false, requested: false, applied: undefined, status: 'unsupported' },
      autoGainControl: { supported: false, requested: false, applied: undefined, status: 'unsupported' },
    },
  })

  it('skips a stage that would duplicate an active native feature', () => {
    expect(selectDspStages([stage('aec', ['echoCancellation'])], diagnostics('active'))).toEqual([])
  })

  it('keeps a stage when the native feature is not active', () => {
    expect(selectDspStages([stage('aec', ['echoCancellation'])], diagnostics('unsupported')).map((s) => s.name)).toEqual(['aec'])
  })

  it('does not let two stages provide the same feature', () => {
    const picked = selectDspStages([stage('a', ['noiseSuppression']), stage('b', ['noiseSuppression'])], diagnostics('active'))
    expect(picked.map((s) => s.name)).toEqual(['a'])
  })
})

describe('pcm-frame-processor worklet', () => {
  type Processor = {
    port: { postMessage: (m: ArrayBuffer, t: ArrayBuffer[]) => void }
    process: (inputs: Float32Array[][]) => boolean
  }

  function load(frameSamples: number) {
    const posted: Float32Array[] = []
    let Ctor: (new (o: unknown) => Processor) | undefined
    class Base {
      port = {
        postMessage: (m: ArrayBuffer) => posted.push(new Float32Array(m)),
        onmessage: null,
      }
    }
    const code = readFileSync(resolve(import.meta.dirname, '../../../../../public/audio/pcm-frame-processor.js'), 'utf8')
    new Function('AudioWorkletProcessor', 'registerProcessor', code)(Base, (_: string, c: typeof Ctor) => (Ctor = c))
    if (!Ctor) throw new Error('not registered')
    return { processor: new Ctor({ processorOptions: { frameSamples } }), posted }
  }

  it('reframes 128-sample render quanta into fixed frames without losing samples', () => {
    const { processor, posted } = load(320)
    let next = 0
    for (let q = 0; q < 10; q++) {
      const quantum = Float32Array.from({ length: 128 }, () => next++)
      expect(processor.process([[quantum]])).toBe(true)
    }
    expect(posted.map((f) => f.length)).toEqual([320, 320, 320, 320])
    expect(posted.flatMap((f) => Array.from(f))).toEqual(Array.from({ length: 1280 }, (_, i) => i))
  })

  it('keeps running through a quantum with no input', () => {
    const { processor, posted } = load(320)
    expect(processor.process([[]])).toBe(true)
    expect(posted).toEqual([])
  })
})
