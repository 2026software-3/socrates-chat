/* global AudioWorkletProcessor, registerProcessor */

/**
 * 把 128 樣本的 render quantum 切成固定長度的幀，轉移給主執行緒（S-05.5）。
 * 放在 public/ 是為了維持同源網址：Vite 可能把小檔案內嵌成 data URL。
 * 只緩衝一幀，不做其他處理，音訊也不會離開瀏覽器。
 */
class PcmFrameProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super()
    this.frameSamples = options.processorOptions.frameSamples
    this.frame = new Float32Array(this.frameSamples)
    this.filled = 0
  }

  process(inputs) {
    const channel = inputs[0]?.[0]
    if (!channel) return true
    let read = 0
    while (read < channel.length) {
      const n = Math.min(channel.length - read, this.frameSamples - this.filled)
      this.frame.set(channel.subarray(read, read + n), this.filled)
      this.filled += n
      read += n
      if (this.filled === this.frameSamples) {
        const { buffer } = this.frame
        this.port.postMessage(buffer, [buffer])
        this.frame = new Float32Array(this.frameSamples)
        this.filled = 0
      }
    }
    return true
  }
}

registerProcessor('pcm-frame-processor', PcmFrameProcessor)
