import { describe, expect, it } from 'vitest'
import { createSpeechDetector } from '@/features/chat/voice/audio/speech-detector'
import { feed, noiseSource, voicedSource } from '@/test/fake-audio'

const CONFIG = { calibrationMs: 600, marginDb: 8, minVoicedMs: 240 }
const HIGH_RISK = { calibrationMs: 1000, marginDb: 14, minVoicedMs: 360 }

function run(sampleRate: number, config = CONFIG) {
  const detector = createSpeechDetector(config)
  const state = { firedAt: undefined as number | undefined, t: 0 }
  const play = (source: (ms: number) => Float32Array, ms: number) =>
    feed(source, ms, (samples) => {
      state.t += 20
      if (detector.push({ samples, sampleRate }) && state.firedAt === undefined) state.firedAt = state.t
    })
  return { play, state }
}

describe.each([16_000, 44_100, 48_000])('speech detector at %i Hz', (rate) => {
  it('fires on sustained voiced speech above the learned level', () => {
    const { play, state } = run(rate)
    play(noiseSource(rate, -60), 800)
    expect(state.firedAt).toBeUndefined()
    play(voicedSource(rate, -25), 400)
    expect(state.firedAt).toBeGreaterThan(800)
    expect(state.firedAt).toBeLessThanOrEqual(800 + 320)
  })

  it('does not fire on loud noise: volume alone is not speech', () => {
    const { play, state } = run(rate)
    play(noiseSource(rate, -60), 800)
    play(noiseSource(rate, -20, 7), 2000)
    expect(state.firedAt).toBeUndefined()
  })
})

describe('speech detector', () => {
  const rate = 48_000

  it('does not fire on a short voiced burst such as a cough or a dropped object', () => {
    const { play, state } = run(rate)
    play(noiseSource(rate, -60), 800)
    play(voicedSource(rate, -20), 160)
    play(noiseSource(rate, -60), 600)
    expect(state.firedAt).toBeUndefined()
  })

  it('does not fire on playback echo it has already learned', () => {
    const { play, state } = run(rate)
    play(voicedSource(rate, -30), 5000)
    expect(state.firedAt).toBeUndefined()
  })

  it('does not fire when echo returns after a pause in the AI speech', () => {
    const { play, state } = run(rate)
    play(voicedSource(rate, -30), 1500)
    play(noiseSource(rate, -65), 1000)
    play(voicedSource(rate, -30), 1500)
    expect(state.firedAt).toBeUndefined()
  })

  it('does not fire when echo returns after a pause longer than the level window', () => {
    const { play, state } = run(rate)
    play(voicedSource(rate, -30), 1500)
    play(noiseSource(rate, -65), 3500)
    play(voicedSource(rate, -30), 1500)
    expect(state.firedAt).toBeUndefined()
  })

  it('does not fire when a high-risk echo returns after a long pause', () => {
    const { play, state } = run(rate, HIGH_RISK)
    play(voicedSource(rate, -30), 2000)
    play(noiseSource(rate, -65), 5000)
    play(voicedSource(rate, -30), 2000)
    expect(state.firedAt).toBeUndefined()
  })

  it('does not fire when the AI voice steps up after a quiet calibration', () => {
    const { play, state } = run(rate)
    play(voicedSource(rate, -40), 800)
    play(voicedSource(rate, -28), 2000)
    expect(state.firedAt).toBeUndefined()
  })

  it('does not fire when a high-risk AI voice steps up after a quiet calibration', () => {
    const { play, state } = run(rate, HIGH_RISK)
    play(voicedSource(rate, -44), 1200)
    play(voicedSource(rate, -24), 2000)
    expect(state.firedAt).toBeUndefined()
  })

  it('follows a gradual rise in the AI voice', () => {
    const { play, state } = run(rate)
    play(voicedSource(rate, -44), 800)
    for (let db = -43; db <= -20; db += 1) play(voicedSource(rate, db), 200)
    expect(state.firedAt).toBeUndefined()
  })

  it('still fires on a different voice right after the AI voice rose', () => {
    const { play, state } = run(rate)
    play(voicedSource(rate, -40), 800)
    play(voicedSource(rate, -30, 230), 500)
    expect(state.firedAt).toBeDefined()
  })

  it('still fires on a same-pitch voice that is far louder than the echo', () => {
    const { play, state } = run(rate)
    play(voicedSource(rate, -40), 800)
    play(voicedSource(rate, -20), 500)
    expect(state.firedAt).toBeDefined()
  })

  it('a cough right after calibration does not blind the rest of the turn', () => {
    const { play, state } = run(rate)
    play(noiseSource(rate, -60), 700)
    play(voicedSource(rate, -20), 160)
    play(noiseSource(rate, -60), 3500)
    play(voicedSource(rate, -25), 500)
    expect(state.firedAt).toBeDefined()
  })

  it('fires when the student talks clearly over residual echo', () => {
    const { play, state } = run(rate)
    play(voicedSource(rate, -35), 1500)
    play(voicedSource(rate, -20, 210), 400)
    expect(state.firedAt).toBeDefined()
  })

  it('does not fire on speech that fails to exceed the echo by the margin', () => {
    const { play, state } = run(rate)
    play(voicedSource(rate, -30), 1500)
    play(voicedSource(rate, -27, 210), 1000)
    expect(state.firedAt).toBeUndefined()
  })

  it('a short sound does not block the speech that follows it', () => {
    const { play, state } = run(rate)
    play(noiseSource(rate, -60), 800)
    play(voicedSource(rate, -20), 160)
    play(noiseSource(rate, -60), 100)
    play(voicedSource(rate, -21), 600)
    expect(state.firedAt).toBeDefined()
  })

  it('fires on syllables broken up by loud consonants', () => {
    const { play, state } = run(rate)
    play(noiseSource(rate, -60), 800)
    const consonant = noiseSource(rate, -22, 3)
    const vowel = voicedSource(rate, -22)
    for (let i = 0; i < 4; i++) {
      play(vowel, 140)
      play(consonant, 80)
    }
    expect(state.firedAt).toBeDefined()
  })

  it('learns a recurring louder sound instead of firing on it later', () => {
    const { play, state } = run(rate)
    play(noiseSource(rate, -60), 800)
    const quiet = noiseSource(rate, -60, 5)
    const blip = voicedSource(rate, -25)
    for (let i = 0; i < 20; i++) {
      play(blip, 160)
      play(quiet, 340)
    }
    expect(state.firedAt).toBeUndefined()
  })

  it('ignores everything during calibration', () => {
    const { play, state } = run(rate)
    play(voicedSource(rate, -10), 500)
    expect(state.firedAt).toBeUndefined()
  })

  it('tolerates a brief unvoiced gap inside the speech', () => {
    const { play, state } = run(rate)
    play(noiseSource(rate, -60), 800)
    play(voicedSource(rate, -25), 140)
    play(noiseSource(rate, -30), 40)
    play(voicedSource(rate, -25), 140)
    expect(state.firedAt).toBeDefined()
  })

  it('stays quiet in digital silence', () => {
    const { play, state } = run(rate)
    play((ms) => new Float32Array((rate * ms) / 1000), 2000)
    expect(state.firedAt).toBeUndefined()
  })

  it('needs an audible absolute level, so near-silence cannot fire', () => {
    const { play, state } = run(rate)
    play((ms) => new Float32Array((rate * ms) / 1000), 800)
    play(voicedSource(rate, -60), 1000)
    expect(state.firedAt).toBeUndefined()
  })

  it('fires only once', () => {
    const detector = createSpeechDetector(CONFIG)
    let fires = 0
    feed(noiseSource(rate, -60), 800, (samples) => void detector.push({ samples, sampleRate: rate }))
    feed(voicedSource(rate, -25), 1000, (samples) => {
      if (detector.push({ samples, sampleRate: rate })) fires += 1
    })
    expect(fires).toBe(1)
  })
})
