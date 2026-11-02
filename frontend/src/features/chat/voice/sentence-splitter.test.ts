import { afterEach, describe, expect, it, vi } from 'vitest'
import { createSentenceSplitter } from '@/features/chat/voice/sentence-splitter'

describe('sentence splitter', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('holds the last sentence until the next one starts', () => {
    const take = createSentenceSplitter('zh-TW')
    expect(take('你為什麼這樣想？', false)).toEqual([])
    expect(take('你為什麼這樣想？如果', false)).toEqual(['你為什麼這樣想？'])
  })

  it('emits each sentence once as the text grows', () => {
    const take = createSentenceSplitter('zh-TW')
    expect(take('第一句。第二句。第', false)).toEqual(['第一句。', '第二句。'])
    expect(take('第一句。第二句。第三句', false)).toEqual([])
    expect(take('第一句。第二句。第三句。', true)).toEqual(['第三句。'])
    expect(take('第一句。第二句。第三句。', true)).toEqual([])
  })

  it('flushes a reply that has no closing punctuation', () => {
    expect(createSentenceSplitter('zh-TW')('沒有句號的回覆', true)).toEqual(['沒有句號的回覆'])
  })

  it('does not split a decimal that arrives across two deltas', () => {
    const take = createSentenceSplitter('en-US')
    expect(take('It is 3.', false)).toEqual([])
    expect(take('It is 3.14 percent. Why', false)).toEqual(['It is 3.14 percent.'])
  })

  it('keeps a closing quote with its sentence', () => {
    const take = createSentenceSplitter('zh-TW')
    expect(take('他說：「為什麼？」你呢', false)).toEqual(['他說：「為什麼？」'])
  })

  it('treats a line break as a boundary', () => {
    const take = createSentenceSplitter('en-US')
    expect(take('First point\nSecond', false)).toEqual(['First point'])
  })

  it('splits Spanish with inverted question marks', () => {
    const take = createSentenceSplitter('es-ES')
    expect(take('Entiendo. ¿Por qué piensas eso? Di', false)).toEqual(['Entiendo.', '¿Por qué piensas eso?'])
  })

  it('skips segments that are only whitespace', () => {
    const take = createSentenceSplitter('en-US')
    expect(take('Done.\n\n   \n', true)).toEqual(['Done.'])
  })

  it('waits for the whole reply when the browser cannot segment', () => {
    vi.stubGlobal('Intl', { ...Intl, Segmenter: undefined })
    const take = createSentenceSplitter('zh-TW')
    expect(take('第一句。第二句。第', false)).toEqual([])
    expect(take('第一句。第二句。第三句。', true)).toEqual(['第一句。第二句。第三句。'])
  })
})
