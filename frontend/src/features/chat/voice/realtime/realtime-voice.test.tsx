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
