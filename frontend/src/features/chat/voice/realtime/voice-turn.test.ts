import { describe, expect, it } from 'vitest'
import { createVoiceTurns } from '@/features/chat/voice/realtime/voice-turn'

async function collect(text: AsyncIterable<string>) {
  const out: string[] = []
  for await (const t of text) out.push(t)
  return out
}

describe('createVoiceTurns', () => {
  it('has nothing to take until a turn begins, and hands each turn out once', () => {
    const turns = createVoiceTurns()
    expect(turns.take()).toBeUndefined()
    turns.begin()
    expect(turns.take()).toBeDefined()
    expect(turns.take()).toBeUndefined()
  })

  it('delivers text pushed before and after the reader started, then ends', async () => {
    const turns = createVoiceTurns()
    const turn = turns.begin()
    turn.push('一')
    const reading = collect(turns.take()!)
    await Promise.resolve()
    turn.push('二')
    turn.end()
    expect(await reading).toEqual(['一', '二'])
  })

  it('ignores text after the end', async () => {
    const turns = createVoiceTurns()
    const turn = turns.begin()
    turn.end()
    turn.push('遲到')
    expect(await collect(turns.take()!)).toEqual([])
  })
})
