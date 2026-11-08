import { describe, expect, it } from 'vitest'
import { createDownsampler, decodePcm16, encodePcm16 } from '@/features/chat/voice/realtime/pcm'

describe('createDownsampler', () => {
  it('produces the target number of samples across frames of any size', () => {
    const down = createDownsampler(44_100, 16_000)
    let total = 0
    for (let i = 0; i < 100; i += 1) total += down(new Float32Array(441)).length
    // 1 秒的輸入；誤差不超過 1 個樣本，幀的邊界不能累積誤差
    expect(Math.abs(total - 16_000)).toBeLessThanOrEqual(1)
  })

  it('keeps the level of a constant signal', () => {
    const down = createDownsampler(48_000, 16_000)
    const out = down(new Float32Array(480).fill(0.5))
    expect(out.length).toBe(160)
    for (const s of out) expect(s).toBeCloseTo(0.5 * 32767, -1)
  })

  it('clips samples outside the 16-bit range', () => {
    const down = createDownsampler(16_000, 16_000)
    expect([...down(new Float32Array([2, -2]))]).toEqual([32767, -32768])
  })

  it('passes audio through when the input rate is already the target', () => {
    const down = createDownsampler(16_000, 16_000)
    expect(down(new Float32Array(160)).length).toBe(160)
  })

  it('averages high frequencies away instead of aliasing them', () => {
    const down = createDownsampler(48_000, 16_000)
    // 接近輸入奈奎斯特頻率的訊號不該以同樣音量留下來
    const tone = Float32Array.from({ length: 480 }, (_, i) => (i % 2 === 0 ? 0.5 : -0.5))
    const peak = Math.max(...[...down(tone)].map(Math.abs))
    expect(peak).toBeLessThan(0.2 * 32767)
  })
})

describe('pcm16 base64', () => {
  it('round-trips samples', () => {
    const samples = Int16Array.from([0, 1, -1, 32767, -32768, 1234])
    expect([...decodePcm16(encodePcm16(samples))]).toEqual([...samples])
  })

  it('encodes as little-endian bytes', () => {
    expect(encodePcm16(Int16Array.from([1]))).toBe('AQA=')
  })

  it('handles frames larger than an argument list can hold', () => {
    const samples = new Int16Array(200_000).fill(7)
    expect(decodePcm16(encodePcm16(samples)).length).toBe(200_000)
  })

  it('ignores a trailing odd byte', () => {
    expect(decodePcm16('AQAC').length).toBe(1)
  })
})
