import { afterEach, describe, expect, it, vi } from 'vitest'
import { MicrophoneError, openMicrophone, statusOf } from '@/features/chat/voice/audio/microphone'
import { ALL_APPLIED, ALL_SUPPORTED, FakeTrack, fakeStream, installMediaDevices, removeMediaDevices } from '@/test/fake-audio'

afterEach(removeMediaDevices)

const domError = (name: string) => new DOMException('x', name)

async function failure(p: Promise<unknown>): Promise<MicrophoneError> {
  const e = await p.then(
    () => undefined,
    (err: unknown) => err,
  )
  expect(e).toBeInstanceOf(MicrophoneError)
  return e as MicrophoneError
}

describe('feature detection and applied settings', () => {
  it('requests all three when the browser lists them, and reports them active when applied', async () => {
    const { getUserMedia } = installMediaDevices()
    const mic = await openMicrophone()
    expect(getUserMedia).toHaveBeenCalledWith({
      audio: { echoCancellation: true, noiseSuppression: true, autoGainControl: true },
    })
    expect(mic.diagnostics.features.echoCancellation).toEqual({ supported: true, requested: true, applied: true, status: 'active' })
    expect(statusOf(mic.diagnostics)).toEqual({ echoCancellation: 'active', noiseSuppression: 'active', autoGainControl: 'active' })
  })

  it('only requests what the browser lists, and marks the rest unsupported', async () => {
    const { getUserMedia } = installMediaDevices({
      supported: { echoCancellation: true },
      settings: { echoCancellation: true },
    })
    const mic = await openMicrophone()
    expect(getUserMedia).toHaveBeenCalledWith({ audio: { echoCancellation: true } })
    expect(statusOf(mic.diagnostics)).toEqual({ echoCancellation: 'active', noiseSuppression: 'unsupported', autoGainControl: 'unsupported' })
  })

  it('asks for a plain microphone when nothing is listed', async () => {
    const { getUserMedia } = installMediaDevices({ supported: {}, settings: {} })
    await openMicrophone()
    expect(getUserMedia).toHaveBeenCalledWith({ audio: true })
  })

  it('does not assume a requested feature is on when settings omit it', async () => {
    installMediaDevices({ settings: { noiseSuppression: true } })
    const mic = await openMicrophone()
    expect(statusOf(mic.diagnostics)).toEqual({ echoCancellation: 'unreported', noiseSuppression: 'active', autoGainControl: 'unreported' })
    expect(mic.diagnostics.features.echoCancellation.applied).toBeUndefined()
  })

  it('reports a listed feature that the browser turned off', async () => {
    installMediaDevices({ settings: { ...ALL_APPLIED, autoGainControl: false } })
    const mic = await openMicrophone()
    expect(mic.diagnostics.features.autoGainControl).toEqual({ supported: true, requested: true, applied: false, status: 'inactive' })
  })

  it('treats echo cancellation limited to remote audio as not covering local playback', async () => {
    installMediaDevices({ settings: { ...ALL_APPLIED, echoCancellation: 'remote-only' as unknown as boolean } })
    const mic = await openMicrophone()
    expect(mic.diagnostics.features.echoCancellation.status).toBe('inactive')
  })

  it('includes track facts useful for troubleshooting but no device identity', async () => {
    installMediaDevices({ settings: { ...ALL_APPLIED, sampleRate: 48000, channelCount: 1, deviceId: 'secret', groupId: 'g' } })
    const mic = await openMicrophone()
    expect(mic.diagnostics.sampleRate).toBe(48000)
    expect(mic.diagnostics.channelCount).toBe(1)
    expect(JSON.stringify(mic.diagnostics)).not.toContain('secret')
  })

  it('still works when getSupportedConstraints is missing', async () => {
    const track = new FakeTrack({})
    const getUserMedia = vi.fn(() => Promise.resolve(fakeStream(track)))
    Object.defineProperty(navigator, 'mediaDevices', { configurable: true, value: { getUserMedia } })
    const mic = await openMicrophone()
    expect(getUserMedia).toHaveBeenCalledWith({ audio: true })
    expect(mic.diagnostics.features.echoCancellation.status).toBe('unsupported')
  })
})

describe('failure classification', () => {
  it.each([
    ['NotAllowedError', 'permission'],
    ['SecurityError', 'permission'],
    ['NotFoundError', 'no-device'],
    ['NotReadableError', 'busy'],
    ['AbortError', 'busy'],
    ['TypeError', 'failed'],
    ['OverconstrainedError', 'failed'],
  ])('%s becomes %s', async (name, kind) => {
    installMediaDevices({ getUserMedia: () => Promise.reject(domError(name)) })
    expect((await failure(openMicrophone())).kind).toBe(kind)
  })

  it('reports unsupported when the API is absent (e.g. an insecure origin)', async () => {
    removeMediaDevices()
    expect((await failure(openMicrophone())).kind).toBe('unsupported')
  })

})

describe('resource lifecycle', () => {
  it('stops the tracks if the caller gave up while the prompt was open', async () => {
    let grant: (s: MediaStream) => void = () => {}
    const track = new FakeTrack(ALL_APPLIED)
    installMediaDevices({ getUserMedia: () => new Promise((r) => (grant = r)) })
    const ctrl = new AbortController()
    const pending = openMicrophone({ signal: ctrl.signal })
    ctrl.abort()
    grant(fakeStream(track))
    expect((await failure(pending)).kind).toBe('aborted')
    expect(track.stop).toHaveBeenCalledTimes(1)
  })

  it('does not call getUserMedia when already aborted', async () => {
    const { getUserMedia } = installMediaDevices()
    const ctrl = new AbortController()
    ctrl.abort()
    expect((await failure(openMicrophone({ signal: ctrl.signal }))).kind).toBe('aborted')
    expect(getUserMedia).not.toHaveBeenCalled()
  })

  it('reports a device disconnection through onEnded', async () => {
    const { tracks } = installMediaDevices()
    const onEnded = vi.fn()
    await openMicrophone({ onEnded })
    tracks[0].unplug()
    expect(onEnded).toHaveBeenCalledTimes(1)
  })

  it('reports a system mute through onMuted', async () => {
    const { tracks } = installMediaDevices()
    const onMuted = vi.fn()
    await openMicrophone({ onMuted })
    tracks[0].muteBySystem()
    expect(onMuted).toHaveBeenCalledTimes(1)
  })

  it('close stops every track once and removes the listeners', async () => {
    const { tracks } = installMediaDevices()
    const onEnded = vi.fn()
    const onMuted = vi.fn()
    const mic = await openMicrophone({ onEnded, onMuted })
    mic.close()
    mic.close()
    expect(tracks[0].stop).toHaveBeenCalledTimes(1)
    expect(tracks[0].listeners.size).toBe(0)
    expect(tracks[0].muteListeners.size).toBe(0)
    tracks[0].unplug()
    tracks[0].muteBySystem()
    expect(onEnded).not.toHaveBeenCalled()
    expect(onMuted).not.toHaveBeenCalled()
  })

  it('stops the stream if it unexpectedly has no audio track', async () => {
    const stop = vi.fn()
    installMediaDevices({
      getUserMedia: () => Promise.resolve({ getTracks: () => [{ stop }], getAudioTracks: () => [] } as unknown as MediaStream),
    })
    expect((await failure(openMicrophone())).kind).toBe('failed')
    expect(stop).toHaveBeenCalledTimes(1)
  })

  it('keeps supported values even if the browser mutates its object later', async () => {
    const supported = { ...ALL_SUPPORTED }
    installMediaDevices({ supported })
    const mic = await openMicrophone()
    supported.echoCancellation = false
    expect(mic.diagnostics.features.echoCancellation.supported).toBe(true)
  })
})
