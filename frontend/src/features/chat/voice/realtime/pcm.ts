/** 瀏覽器音訊與 cascade 的 16 位元 PCM 之間的轉換。全部目標平台都是小端序。 */

const BASE64_CHUNK = 0x8000

function toInt16(sample: number): number {
  const clipped = Math.max(-1, Math.min(1, sample))
  return Math.round(clipped < 0 ? clipped * 32768 : clipped * 32767)
}

/**
 * 回傳把 `fromRate` 的音訊降到 `toRate` 的函式，狀態跨幀保留，幀的邊界不會累積誤差。
 * 以區間平均兼作低通濾波；這是給語音辨識的，不追求音質。
 */
export function createDownsampler(fromRate: number, toRate: number): (samples: Float32Array) => Int16Array {
  const step = fromRate / toRate
  let sum = 0
  let count = 0
  let consumed = 0
  let boundary = step
  return (samples) => {
    const out = new Int16Array(Math.ceil(samples.length / step) + 1)
    let written = 0
    for (const sample of samples) {
      sum += sample
      count += 1
      consumed += 1
      if (consumed >= boundary) {
        out[written] = toInt16(sum / count)
        written += 1
        sum = 0
        count = 0
        boundary += step
      }
    }
    return out.subarray(0, written)
  }
}

export function encodePcm16(samples: Int16Array): string {
  const bytes = new Uint8Array(samples.buffer, samples.byteOffset, samples.byteLength)
  let binary = ''
  for (let i = 0; i < bytes.length; i += BASE64_CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + BASE64_CHUNK))
  }
  return btoa(binary)
}

export function decodePcm16(base64: string): Int16Array {
  const binary = atob(base64)
  const bytes = new Uint8Array(binary.length & ~1)
  for (let i = 0; i < bytes.length; i += 1) bytes[i] = binary.charCodeAt(i)
  return new Int16Array(bytes.buffer)
}
