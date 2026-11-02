import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { assessEchoRisk, createBargeInMonitor, type BargeInStatus } from '@/features/chat/voice/audio/barge-in'
import type { MicrophoneDiagnostics } from '@/features/chat/voice/audio/microphone'
import {
  ALL_APPLIED,
  FakeAudioContext,
  FakeTrack,
  FakeWorkletNode,
  fakeStream,
  feed,
  installAudioGraph,
  installMediaDevices,
  noiseSource,
  removeMediaDevices,
  voicedSource,
} from '@/test/fake-audio'

const tick = () => new Promise((r) => setTimeout(r, 0))
const settle = async () => {
  for (let i = 0; i < 5; i++) await tick()
}

function monitor() {
  const onSpeech = vi.fn()
  const statuses: BargeInStatus[] = []
  const m = createBargeInMonitor({ onSpeech, onStatus: (s) => statuses.push(s) })
  return { m, onSpeech, statuses, last: () => statuses[statuses.length - 1] }
}

function speakIntoMic() {
  const node = FakeWorkletNode.last
  const emit = (s: Float32Array) => node.emit(s)
  feed(noiseSource(48_000, -60), 1200, emit)
  feed(voicedSource(48_000, -20), 600, emit)
}

let warn: ReturnType<typeof vi.spyOn>

beforeEach(() => {
  installAudioGraph()
  warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
})
afterEach(() => {
  removeMediaDevices()
  vi.unstubAllGlobals()
  warn.mockRestore()
})

describe('assessEchoRisk', () => {
  const diag = (echoCancellation: boolean | undefined): MicrophoneDiagnostics => ({
    features: {
      echoCancellation: {
        supported: true,
        requested: true,
        applied: echoCancellation,
        status: echoCancellation === undefined ? 'unreported' : echoCancellation ? 'active' : 'inactive',
      },
      noiseSuppression: { supported: true, requested: true, applied: true, status: 'active' },
      autoGainControl: { supported: true, requested: true, applied: true, status: 'active' },
    },
  })

  it('is low only for browser-rendered audio with echo cancellation reported on', () => {
    expect(assessEchoRisk('element', diag(true))).toBe('low')
  })

  it('stays high for speechSynthesis even when echo cancellation is reported on', () => {
    expect(assessEchoRisk('synthesis', diag(true))).toBe('high')
  })

  it('stays high when echo cancellation is off or unreported', () => {
    expect(assessEchoRisk('element', diag(false))).toBe('high')
    expect(assessEchoRisk('element', diag(undefined))).toBe('high')
  })
})

describe('barge-in monitor', () => {
  it('keeps capturing during playback and reports diagnostics', async () => {
    const { tracks } = installMediaDevices()
    const { m, last } = monitor()
    m.unlock()
    m.start('element')
    await settle()
    expect(last()).toMatchObject({ state: 'starting', echoRisk: 'low' })
    FakeWorkletNode.last.emit(new Float32Array(960))
    expect(last()).toMatchObject({ state: 'monitoring', echoRisk: 'low' })
    expect(last().microphone?.features.echoCancellation.status).toBe('active')
    expect(tracks[0].readyState).toBe('live')
    expect(FakeAudioContext.last.resume).toHaveBeenCalled()
  })

  it('fires once on speech and releases the microphone first', async () => {
    const { tracks } = installMediaDevices()
    const { m, onSpeech } = monitor()
    let liveWhenFired: string | undefined
    onSpeech.mockImplementation(() => (liveWhenFired = tracks[0].readyState))
    m.unlock()
    m.start('element')
    await settle()
    speakIntoMic()
    expect(onSpeech).toHaveBeenCalledTimes(1)
    expect(liveWhenFired).toBe('ended')
  })

  it('does not keep the context running while nothing is being monitored', async () => {
    installMediaDevices()
    const { m } = monitor()
    m.unlock()
    expect(FakeAudioContext.last.resume).toHaveBeenCalledTimes(1) // 手勢內真的喚醒過
    await settle()
    expect(FakeAudioContext.last.state).toBe('suspended')
  })

  it('leaves the context alone if monitoring began while unlocking', async () => {
    installMediaDevices()
    const { m } = monitor()
    m.unlock()
    m.start('element')
    await settle()
    expect(FakeAudioContext.last.state).toBe('running')
  })

  it('resumes even when the context still looks running during a pending suspend', async () => {
    installMediaDevices()
    const { m } = monitor()
    m.start('element')
    await settle()
    const ctx = FakeAudioContext.last
    // suspend 還沒生效：state 維持 running，順序全靠 Promise 佇列
    ctx.suspend.mockImplementationOnce(() => Promise.resolve())
    ctx.resume.mockClear()
    m.stop()
    m.start('element')
    await settle()
    expect(ctx.resume).toHaveBeenCalled()
  })

  it('does not claim to be listening until audio really flows', async () => {
    installMediaDevices()
    const { m, statuses } = monitor()
    m.start('element')
    await settle()
    expect(statuses.map((s) => s.state)).not.toContain('monitoring')
    const updates = statuses.length
    FakeWorkletNode.last.emit(new Float32Array(960))
    FakeWorkletNode.last.emit(new Float32Array(960))
    FakeWorkletNode.last.emit(new Float32Array(960))
    // 只有第一幀會更新狀態：不能每幀都觸發畫面更新
    expect(statuses).toHaveLength(updates + 1)
  })

  it('backs off for the session when the system mutes the microphone', async () => {
    const { tracks, getUserMedia } = installMediaDevices()
    const { m, last, onSpeech } = monitor()
    m.start('element')
    await settle()
    tracks[0].muteBySystem()
    expect(tracks[0].readyState).toBe('ended')
    expect(last()).toMatchObject({ state: 'failed', failure: 'muted' })
    expect(warn).toHaveBeenCalled()
    m.start('element')
    await settle()
    expect(getUserMedia).toHaveBeenCalledTimes(1)
    expect(onSpeech).not.toHaveBeenCalled()
  })

  it('tries again in a new session after a system mute', async () => {
    const { tracks, getUserMedia } = installMediaDevices()
    const { m } = monitor()
    m.start('element')
    await settle()
    tracks[0].muteBySystem()
    m.close()
    m.start('element')
    await settle()
    expect(getUserMedia).toHaveBeenCalledTimes(2)
  })

  it('needs more evidence when echo risk is high', async () => {
    installMediaDevices({ settings: { ...ALL_APPLIED, echoCancellation: false } })
    const { m, onSpeech, last } = monitor()
    m.start('element')
    await settle()
    expect(last().echoRisk).toBe('high')
    const emit = (s: Float32Array) => FakeWorkletNode.last.emit(s)
    feed(voicedSource(48_000, -35), 1500, emit)
    feed(voicedSource(48_000, -26, 210), 800, emit) // 比殘響高 9 dB：低風險會觸發，高風險不會
    expect(onSpeech).not.toHaveBeenCalled()
  })

  it('stop releases the microphone and detaches the graph', async () => {
    const { tracks } = installMediaDevices()
    const { m, last } = monitor()
    m.start('element')
    await settle()
    const node = FakeWorkletNode.last
    m.stop()
    expect(tracks[0].stop).toHaveBeenCalledTimes(1)
    expect(node.disconnect).toHaveBeenCalled()
    expect(FakeAudioContext.last.suspend).toHaveBeenCalled()
    expect(last().state).toBe('idle')
    expect(last().microphone).toBeDefined() // 診斷留著，方便事後查看
  })

  it('survives rapid start and stop without leaking tracks or nodes', async () => {
    const pending: ((s: MediaStream) => void)[] = []
    const tracks: FakeTrack[] = []
    installMediaDevices({ getUserMedia: () => new Promise((r) => pending.push(r)) })
    const { m, onSpeech } = monitor()
    for (let i = 0; i < 5; i++) {
      m.start('element')
      m.stop()
    }
    m.start('element')
    for (const grant of pending) {
      const t = new FakeTrack(ALL_APPLIED)
      tracks.push(t)
      grant(fakeStream(t))
    }
    await settle()
    expect(tracks.filter((t) => t.readyState === 'live')).toHaveLength(1)
    expect(tracks.filter((t) => t.stop.mock.calls.length > 0)).toHaveLength(5)
    expect(FakeWorkletNode.instances).toHaveLength(1)
    expect(onSpeech).not.toHaveBeenCalled()
  })

  it('a stale run does not fire after a newer one started', async () => {
    installMediaDevices()
    const { m, onSpeech } = monitor()
    m.start('element')
    await settle()
    const stale = FakeWorkletNode.last
    m.start('element')
    await settle()
    stale.emit(new Float32Array(960))
    feed(voicedSource(48_000, -20), 2000, (s) => stale.emit(s))
    expect(onSpeech).not.toHaveBeenCalled()
  })

  it('does not resurrect the monitor when stop happens during module load', async () => {
    const { tracks } = installMediaDevices()
    const { m, last } = monitor()
    m.unlock()
    let loaded: () => void = () => {}
    FakeAudioContext.last.audioWorklet.addModule.mockImplementationOnce(() => new Promise<void>((r) => (loaded = r)))
    m.start('element')
    await settle()
    expect(tracks).toHaveLength(1)
    m.stop()
    loaded()
    await settle()
    expect(tracks[0].readyState).toBe('ended')
    expect(FakeWorkletNode.instances).toHaveLength(0)
    expect(last().state).toBe('idle')
  })

  it('reconnects once after the device disappears', async () => {
    const { tracks, getUserMedia } = installMediaDevices()
    const { m, last } = monitor()
    m.start('element')
    await settle()
    const node = FakeWorkletNode.last
    tracks[0].unplug()
    await settle()
    expect(getUserMedia).toHaveBeenCalledTimes(2)
    expect(node.disconnect).toHaveBeenCalled()
    FakeWorkletNode.last.emit(new Float32Array(960))
    expect(last().state).toBe('monitoring')
  })

  it('gives up without throwing when the device is gone for good', async () => {
    let calls = 0
    const track = new FakeTrack(ALL_APPLIED)
    installMediaDevices({
      getUserMedia: () => (calls++ === 0 ? Promise.resolve(fakeStream(track)) : Promise.reject(new DOMException('x', 'NotFoundError'))),
    })
    const { m, last } = monitor()
    m.start('element')
    await settle()
    track.unplug()
    await settle()
    expect(last()).toMatchObject({ state: 'failed', failure: 'no-device' })
    expect(warn).toHaveBeenCalled()
  })

  it('reports denial, warns, and does not prompt again for the session', async () => {
    const { getUserMedia } = installMediaDevices({ getUserMedia: () => Promise.reject(new DOMException('x', 'NotAllowedError')) })
    const { m, last } = monitor()
    m.start('element')
    await settle()
    expect(last()).toMatchObject({ state: 'failed', failure: 'permission' })
    expect(warn).toHaveBeenCalled()
    m.start('element')
    await settle()
    expect(getUserMedia).toHaveBeenCalledTimes(1)
  })

  it('releases the microphone when the audio graph cannot be built', async () => {
    const { tracks } = installMediaDevices()
    vi.stubGlobal('AudioWorkletNode', undefined)
    const { m, last } = monitor()
    m.start('element')
    await settle()
    expect(last()).toMatchObject({ state: 'failed', failure: 'capture' })
    expect(tracks[0].readyState).toBe('ended')
  })

  it('reports unsupported when there is no AudioContext', async () => {
    installMediaDevices()
    vi.stubGlobal('AudioContext', undefined)
    const { m, last } = monitor()
    m.unlock()
    m.start('element')
    await settle()
    expect(last()).toMatchObject({ state: 'failed', failure: 'unsupported' })
  })

  it('reports a failed context resume instead of swallowing it', async () => {
    installMediaDevices()
    const { m } = monitor()
    m.unlock()
    FakeAudioContext.last.state = 'suspended'
    FakeAudioContext.last.resume.mockImplementationOnce(() => Promise.reject(new Error('blocked')))
    m.start('element')
    await settle()
    expect(warn).toHaveBeenCalled()
  })

  it('close releases everything and the context, and can start over', async () => {
    const { tracks } = installMediaDevices()
    const { m } = monitor()
    m.unlock()
    m.start('element')
    await settle()
    const ctx = FakeAudioContext.last
    m.close()
    m.close()
    expect(tracks[0].readyState).toBe('ended')
    expect(ctx.close).toHaveBeenCalledTimes(1)
    m.unlock()
    expect(FakeAudioContext.instances).toHaveLength(2)
  })
})
