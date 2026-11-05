import { act, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { Route, Routes } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { ConversationPage } from '@/features/chat/conversation-page'
import { makeDetail, makeMessage } from '@/features/chat/test-data'
import { createRealtimeSession, type RealtimeCallbacks } from '@/features/chat/voice/realtime/realtime-session'
import { AI_USAGE_STALE_EVENT } from '@/lib/ai-usage-events'
import type { ConversationDetail } from '@/lib/types'
import { makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

vi.mock('@/features/chat/voice/realtime/realtime-session', async (original) => ({
  ...(await original<typeof import('@/features/chat/voice/realtime/realtime-session')>()),
  createRealtimeSession: vi.fn(),
}))

const session = { interrupt: vi.fn(), setInterruptible: vi.fn(), close: vi.fn() }
const posts: unknown[] = []

function renderPage(detail: ConversationDetail) {
  server.use(
    http.get('/api/conversations/c1', () => HttpResponse.json(detail)),
    http.post('/api/conversations/c1/messages', async ({ request }) => {
      posts.push(await request.json())
      return HttpResponse.json({}, { status: 500 })
    }),
  )
  mockMe(makeMe({ student: true }, { language: 'zh-TW' }))
  return renderApp(
    <Routes>
      <Route path="/conversations/:id" element={<ConversationPage pollMs={5} maxPollMs={400} />} />
    </Routes>,
    { route: '/conversations/c1' },
  )
}

const startButton = () => screen.findByRole('button', { name: '語音對話' })
const callbacks = (): RealtimeCallbacks => vi.mocked(createRealtimeSession).mock.calls.at(-1)![1]

async function startLive() {
  const user = userEvent.setup()
  renderPage(makeDetail({ voice_realtime: true }))
  await user.click(await startButton())
  return { user, cb: callbacks() }
}

describe('server voice conversation', () => {
  beforeEach(() => {
    posts.length = 0
    vi.mocked(createRealtimeSession).mockReset()
    vi.mocked(createRealtimeSession).mockReturnValue(session)
  })

  afterEach(() => {
    vi.clearAllMocks()
  })

  it('is not used when the server has it off', async () => {
    const user = userEvent.setup()
    renderPage(makeDetail({ voice_realtime: false }))
    await user.click(await startButton())
    expect(createRealtimeSession).not.toHaveBeenCalled()
  })

  it('is not used for languages it cannot handle yet', async () => {
    const user = userEvent.setup()
    renderPage(makeDetail({ voice_realtime: true, language: 'en' }))
    await user.click(await startButton())
    expect(createRealtimeSession).not.toHaveBeenCalled()
  })

  it('tells the student where the voice goes and locks typing while connected', async () => {
    await startLive()
    expect(createRealtimeSession).toHaveBeenCalledWith('c1', expect.anything(), false)
    expect(screen.getByText(/即時傳到本系統的伺服器/)).toBeInTheDocument()
    expect(screen.getByRole('textbox')).toBeDisabled()
    expect(screen.getByRole('button', { name: '送出' })).toBeDisabled()
  })

  it('shows a voice turn in the thread without posting it', async () => {
    const { cb } = await startLive()
    act(() => {
      cb.onPhase('listening')
      cb.onTranscript('合成的語音發言')
    })
    expect(await screen.findByText('合成的語音發言')).toBeInTheDocument()
    act(() => cb.onReplyDelta('那你認為'))
    act(() => cb.onReplyDelta('為什麼？'))
    expect(await screen.findByText('那你認為為什麼？')).toBeInTheDocument()
    act(() => cb.onReplyEnd())
    await waitFor(() => expect(screen.getByText('回合：2')).toBeInTheDocument())
    expect(posts).toEqual([])
  })

  it('puts an interrupted utterance and the reply to the next one in order', async () => {
    const { cb } = await startLive()
    const aiBubbles = screen.getAllByText('蘇格拉底').length
    act(() => {
      cb.onPhase('listening')
      cb.onTranscript('甲')
    })
    expect(await screen.findByText('甲')).toBeInTheDocument()

    // 第二句辨識完成時 session 先打斷還開著的回覆（收尾），第二句馬上出現，回覆接在它下面
    act(() => {
      cb.onReplyEnd()
      cb.onTranscript('乙')
    })
    expect(await screen.findByText('乙')).toBeInTheDocument()
    act(() => cb.onReplyDelta('回覆'))
    const reply = await screen.findByText('回覆')
    const first = screen.getByText('甲')
    const second = screen.getByText('乙')
    expect(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(second.compareDocumentPosition(reply) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    // 被打斷、沒有任何文字的那一輪不留空的 AI 泡泡
    expect(screen.getAllByText('蘇格拉底')).toHaveLength(aiBubbles + 1)
    act(() => cb.onReplyEnd())
  })

  it('counts an interrupted utterance and its follow-up as one turn in the header', async () => {
    const { cb } = await startLive()
    expect(screen.getByText('回合：1')).toBeInTheDocument()
    act(() => {
      cb.onPhase('listening')
      cb.onTranscript('甲')
    })
    expect(await screen.findByText('回合：2')).toBeInTheDocument()

    // 甲的回覆一個字都還沒有就被乙打斷：後端把乙存成獨立訊息但不加回合
    act(() => {
      cb.onReplyEnd()
      cb.onTranscript('乙')
    })
    expect(await screen.findByText('乙')).toBeInTheDocument()
    expect(screen.getByText('回合：2')).toBeInTheDocument()
    act(() => cb.onReplyEnd())
  })

  it('takes the turn count from the server after a turn, even when text arrived but was never heard', async () => {
    const user = userEvent.setup()
    renderPage(makeDetail({ voice_realtime: true, turn_count: 1 }))
    await user.click(await startButton())
    const cb = callbacks()
    act(() => {
      cb.onPhase('listening')
      cb.onTranscript('甲')
    })
    // 甲的回覆已有文字，但學生還沒聽到就被乙打斷：畫面無從得知，後端不為乙加回合
    act(() => cb.onReplyDelta('回'))
    act(() => {
      cb.onReplyEnd()
      cb.onTranscript('乙')
    })
    expect(await screen.findByText('回合：3')).toBeInTheDocument()

    server.use(http.get('/api/conversations/c1', () => HttpResponse.json(makeDetail({ voice_realtime: true, turn_count: 2 }))))
    act(() => cb.onSettled())
    expect(await screen.findByText('回合：2')).toBeInTheDocument()
    act(() => cb.onReplyEnd())
  })

  it('takes stage and ending from the server after a turn', async () => {
    const { cb } = await startLive()
    server.use(http.get('/api/conversations/c1', () => HttpResponse.json(makeDetail({ status: 'ended', voice_realtime: true }))))
    server.use(http.get('/api/conversations/c1/summary', () => HttpResponse.json({ status: 'generating' })))
    act(() => cb.onSettled())
    await waitFor(() => expect(screen.queryByRole('textbox')).not.toBeInTheDocument())
    expect(session.close).toHaveBeenCalled()
  })

  it('refreshes the usage meter after a turn', async () => {
    const { cb } = await startLive()
    const stale = vi.fn()
    window.addEventListener(AI_USAGE_STALE_EVENT, stale)
    act(() => cb.onSettled())
    window.removeEventListener(AI_USAGE_STALE_EVENT, stale)
    expect(stale).toHaveBeenCalledTimes(1)
  })

  it('cannot start while the last student message is unanswered', async () => {
    const messages = [makeMessage({ id: 'm1', role: 'student', content: '還沒有回覆的訊息' })]
    renderPage(makeDetail({ voice_realtime: true }, messages))
    expect(await startButton()).toBeDisabled()
    expect(await screen.findByRole('button', { name: '重試' })).toBeInTheDocument()
  })

  it('the speak button interrupts the AI', async () => {
    const { user, cb } = await startLive()
    act(() => cb.onPhase('speaking'))
    await user.click(screen.getByRole('button', { name: '說話' }))
    expect(session.interrupt).toHaveBeenCalled()
  })

  it('passes the barge-in switch to the session', async () => {
    const { user } = await startLive()
    await user.click(screen.getByRole('switch', { name: /打斷/ }))
    expect(session.setInterruptible).toHaveBeenLastCalledWith(true)
  })

  it('ends with the button and frees the input', async () => {
    const { user } = await startLive()
    await user.click(screen.getByRole('button', { name: '結束語音對話' }))
    expect(session.close).toHaveBeenCalled()
    expect(screen.getByRole('textbox')).toBeEnabled()
  })

  it('falls back to browser voice when the service cannot be reached, and stops trying', async () => {
    const { user, cb } = await startLive()
    act(() => cb.onClosed('unreachable'))
    expect(await screen.findByText(/已改用瀏覽器語音/)).toBeInTheDocument()
    // jsdom 沒有語音辨識，瀏覽器語音會直接結束；再按一次也不會重連
    await user.click(await startButton())
    expect(createRealtimeSession).toHaveBeenCalledTimes(1)
  })

  it('marks a failed turn for retry and stops the voice session', async () => {
    const { cb } = await startLive()
    act(() => {
      cb.onPhase('listening')
      cb.onTranscript('合成的語音發言')
    })
    await screen.findByText('合成的語音發言')
    act(() => cb.onReplyDelta('唸到一半'))
    await screen.findByText('唸到一半')
    act(() => {
      cb.onReplyFailed()
      cb.onReplyEnd()
      cb.onClosed('replyFailed')
    })
    expect(await screen.findByText(/這一輪沒有成功/)).toBeInTheDocument()
    expect(await screen.findByRole('button', { name: '重試' })).toBeInTheDocument()
    expect(await startButton()).toBeDisabled()
  })

  it('offers a restart after a dropped connection', async () => {
    const { cb } = await startLive()
    act(() => cb.onClosed('lost'))
    expect(await screen.findByText(/語音連線中斷了/)).toBeInTheDocument()
    expect(screen.getByRole('textbox')).toBeEnabled()
    expect(await startButton()).toBeEnabled()
  })

  it('explains a denied microphone without falling back', async () => {
    const { cb } = await startLive()
    act(() => cb.onClosed('permission'))
    expect(await screen.findByText(/請在瀏覽器允許麥克風權限/)).toBeInTheDocument()
    expect(screen.queryByText(/已改用瀏覽器語音/)).not.toBeInTheDocument()
  })

})
