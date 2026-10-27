import { afterEach, describe, expect, it, vi } from 'vitest'
import { translateInBrowser } from '@/lib/browser-translator'

type Availability = 'unavailable' | 'downloadable' | 'downloading' | 'available'

function stubTranslator(availability: Availability) {
  const translate = vi.fn(async (text: string) => `[${text}]`)
  const destroy = vi.fn()
  const create = vi.fn(async () => ({ translate, destroy }))
  vi.stubGlobal('Translator', { availability: vi.fn(async () => availability), create })
  return { translate, destroy, create }
}

describe('translateInBrowser', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('translates with the on-device translator when the language pack is installed', async () => {
    const stub = stubTranslator('available')
    expect(await translateInBrowser('zh-TW', 'en', ['立場', '理由'])).toEqual(['[立場]', '[理由]'])
    expect(stub.destroy).toHaveBeenCalled()
  })

  it('never sends empty text to the translator and keeps its position', async () => {
    const stub = stubTranslator('available')
    expect(await translateInBrowser('zh-TW', 'en', ['', '理由', '  '])).toEqual(['', '[理由]', ''])
    expect(stub.translate).toHaveBeenCalledTimes(1)
    expect(stub.translate).toHaveBeenCalledWith('理由')
  })

  it('does not even create a translator when every text is empty', async () => {
    const stub = stubTranslator('available')
    expect(await translateInBrowser('zh-TW', 'en', ['', ''])).toEqual(['', ''])
    expect(stub.create).not.toHaveBeenCalled()
  })

  it.each<Availability>(['downloading', 'unavailable'])(
    'falls back to the server at once when the pack is %s without touching create()',
    async (availability) => {
      const stub = stubTranslator(availability)
      expect(await translateInBrowser('zh-TW', 'en', ['立場', '理由'])).toBeNull()
      expect(stub.create).not.toHaveBeenCalled()
    },
  )

  it('starts the pack download in the background when it is downloadable, but does not wait for it', async () => {
    const stub = stubTranslator('downloadable')
    // 下載要很久：create() 在這個測試裡永遠不會結束
    stub.create.mockImplementation(() => new Promise(() => {}))
    expect(await translateInBrowser('zh-TW', 'en', ['立場', '理由'])).toBeNull()
    expect(stub.create).toHaveBeenCalledTimes(1)
  })

  it('releases the translator once a background download finishes, and ignores a failed one', async () => {
    const stub = stubTranslator('downloadable')
    expect(await translateInBrowser('zh-TW', 'en', ['立場'])).toBeNull()
    await vi.waitFor(() => expect(stub.destroy).toHaveBeenCalled())

    const failing = stubTranslator('downloadable')
    failing.create.mockRejectedValue(new DOMException('no activation', 'NotAllowedError'))
    expect(await translateInBrowser('zh-TW', 'en', ['立場'])).toBeNull()
    await vi.waitFor(() => expect(failing.create).toHaveBeenCalled())
  })

  it('does not start a download when there is nothing to translate', async () => {
    const stub = stubTranslator('downloadable')
    expect(await translateInBrowser('zh-TW', 'en', ['', ' '])).toEqual(['', ''])
    expect(stub.create).not.toHaveBeenCalled()
  })

  it('returns null when the browser has no Translator API', async () => {
    expect(await translateInBrowser('zh-TW', 'en', ['立場'])).toBeNull()
  })
})
