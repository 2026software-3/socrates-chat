import { act, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { Link, Route, Routes } from 'react-router'
import { describe, expect, it, vi } from 'vitest'
import { ConversationPage } from '@/features/chat/conversation-page'
import { makeDetail, makeMessage, makeSummary } from '@/features/chat/test-data'
import type { Lang } from '@/i18n/define'
import type { ConversationDetail, StreamDone, SummaryView } from '@/lib/types'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const enc = new TextEncoder()

function sse(...frames: string[]) {
  const body = new ReadableStream<Uint8Array>({
    start(c) {
      for (const f of frames) c.enqueue(enc.encode(f))
      c.close()
    },
  })
  return new HttpResponse(body, { headers: { 'Content-Type': 'text/event-stream' } })
}
const delta = (text: string) => `event: delta\ndata: ${JSON.stringify({ text })}\n\n`
const doneFrame = (over: Partial<StreamDone> = {}) =>
  `event: done\ndata: ${JSON.stringify({ message_id: 'ai-new', question_type: 'secret_type', stage: 1, suggest_end: false, ...over })}\n\n`
const errorFrame = (code: string) => `event: error\ndata: ${JSON.stringify({ code })}\n\n`

const sentMessage = (auto_ended = false) => ({
  ...makeMessage({ id: 'srv-1', content: '新的想法' }),
  auto_ended,
})

function mockDetail(detail: ConversationDetail | ReturnType<typeof apiError>) {
  server.use(http.get('/api/conversations/c1', () => ('messages' in detail ? HttpResponse.json(detail) : detail)))
}

/** 依序回傳 summary；最後一個會一直重複。 */
function mockSummaries(...list: SummaryView[]) {
  let i = 0
  const calls = { count: 0 }
  server.use(
    http.get('/api/conversations/c1/summary', () => {
      calls.count += 1
      return HttpResponse.json(list[Math.min(i++, list.length - 1)])
    }),
  )
  return calls
}

function renderPage(opts: { lang?: Lang } = {}) {
  mockMe(makeMe({ student: true }))
  return renderApp(
    <Routes>
      <Route path="/conversations/:id" element={<ConversationPage pollMs={5} maxPollMs={400} />} />
    </Routes>,
    { route: '/conversations/c1', lang: opts.lang },
  )
}

async function send(text: string) {
  const input = await screen.findByRole('textbox', { name: '訊息輸入' })
  await userEvent.type(input, text)
  await userEvent.click(screen.getByRole('button', { name: '送出' }))
}

describe('ConversationPage: loading', () => {
  it('shows the topic, stage, status and the loaded history', async () => {
    mockDetail(makeDetail({ stage: 2 }))
    renderPage()
    expect(await screen.findByRole('heading', { name: '合成題目：電車難題' })).toBeInTheDocument()
    expect(screen.getByText('合成描述：該不該扳動拉桿？')).toBeInTheDocument()
    expect(screen.getByText('進行中')).toBeInTheDocument()
    const stage = screen.getByTestId('stage-indicator')
    expect(within(stage).getByText('論證').closest('li')).toHaveAttribute('aria-current', 'step')
    expect(within(stage).getByText('釐清').closest('li')).not.toHaveAttribute('aria-current')
    expect(screen.getByText('我認為應該扳動拉桿。')).toBeInTheDocument()
    expect(screen.getByText('你說的「應該」是依據什麼原則呢？')).toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: '訊息輸入' })).toBeInTheDocument()
  })

  it('never shows question_type or other metadata', async () => {
    mockDetail(makeDetail())
    renderPage()
    await screen.findByText('你說的「應該」是依據什麼原則呢？')
    expect(screen.queryByText('clarify')).not.toBeInTheDocument()
  })

  it('shows "not found" for a 404 (also means not yours)', async () => {
    mockDetail(apiError(404, 'not_found'))
    renderPage()
    expect(await screen.findByText('找不到這場對話，或它不屬於你。')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: '回到我的對話' })).toHaveAttribute('href', '/conversations')
  })

  it('shows a translated error with retry for other failures', async () => {
    mockDetail(apiError(500, 'internal'))
    renderPage()
    expect(await screen.findByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
    mockDetail(makeDetail())
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByRole('heading', { name: '合成題目：電車難題' })).toBeInTheDocument()
  })

  it('drops the previous conversation when the route param changes', async () => {
    mockDetail(makeDetail())
    server.use(http.get('/api/conversations/c2', () => apiError(404, 'not_found')))
    mockMe(makeMe({ student: true }))
    renderApp(
      <Routes>
        <Route
          path="/conversations/:id"
          element={
            <>
              <Link to="/conversations/c2">go</Link>
              <ConversationPage />
            </>
          }
        />
      </Routes>,
      { route: '/conversations/c1' },
    )
    await screen.findByRole('heading', { name: '合成題目：電車難題' })
    await userEvent.click(screen.getByRole('link', { name: 'go' }))
    expect(await screen.findByText('找不到這場對話，或它不屬於你。')).toBeInTheDocument()
    expect(screen.queryByRole('heading', { name: '合成題目：電車難題' })).not.toBeInTheDocument()
  })

  it('renders the chat labels in Spanish', async () => {
    mockDetail(makeDetail({ stage: 1 }))
    renderPage({ lang: 'es' })
    expect(await screen.findByRole('textbox', { name: 'Escribir mensaje' })).toBeInTheDocument()
    expect(screen.getByText('Aclarar').closest('li')).toHaveAttribute('aria-current', 'step')
  })

  it('translates the labels to the active language', async () => {
    mockDetail(makeDetail({ stage: 3 }))
    renderPage({ lang: 'en' })
    expect(await screen.findByText('In progress')).toBeInTheDocument()
    expect(screen.getByText('Challenge').closest('li')).toHaveAttribute('aria-current', 'step')
    expect(screen.getByRole('textbox', { name: 'Message input' })).toBeInTheDocument()
  })
})

describe('ConversationPage: sending', () => {
  it('POSTs the message then renders the streamed reply and updates the stage', async () => {
    mockDetail(makeDetail())
    const posts: unknown[] = []
    server.use(
      http.post('/api/conversations/c1/messages', async ({ request }) => {
        posts.push(await request.json())
        return HttpResponse.json(sentMessage(), { status: 201 })
      }),
      http.get('/api/conversations/c1/stream', () =>
        sse(delta('那你'), delta('為什麼這樣想？'), doneFrame({ stage: 2 })),
      ),
    )
    renderPage()
    await send('新的想法')
    expect(await screen.findByText('那你為什麼這樣想？')).toBeInTheDocument()
    expect(screen.getByText('新的想法')).toBeInTheDocument()
    expect(posts).toEqual([{ content: '新的想法', source: 'text' }])
    await waitFor(() =>
      expect(within(screen.getByTestId('stage-indicator')).getByText('論證').closest('li')).toHaveAttribute(
        'aria-current',
        'step',
      ),
    )
    // 只顯示 AI 回覆文字，不顯示 question_type
    expect(screen.queryByText('secret_type')).not.toBeInTheDocument()
    expect(screen.getByRole('textbox', { name: '訊息輸入' })).toHaveValue('')
  })

  it('does not send an empty message', async () => {
    mockDetail(makeDetail())
    renderPage()
    await screen.findByRole('textbox', { name: '訊息輸入' })
    expect(screen.getByRole('button', { name: '送出' })).toBeDisabled()
  })

  it('shows a translated error when the message is rejected, and keeps the conversation usable', async () => {
    mockDetail(makeDetail())
    server.use(http.post('/api/conversations/c1/messages', () => apiError(409, 'reply_in_progress')))
    renderPage()
    await send('新的想法')
    expect(await screen.findByText('AI 正在回覆，請稍候。')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '重試' })).toBeInTheDocument()
  })

  it('blocks a new message while the previous one failed, so an unsaved message is never followed by another', async () => {
    mockDetail(makeDetail())
    let posts = 0
    server.use(
      http.post('/api/conversations/c1/messages', () => {
        posts += 1
        return posts === 1 ? apiError(409, 'reply_in_progress') : HttpResponse.json(sentMessage(), { status: 201 })
      }),
      http.get('/api/conversations/c1/stream', () => sse(delta('補上'), doneFrame())),
    )
    renderPage()
    await send('第一則')
    await screen.findByText('AI 正在回覆，請稍候。')
    expect(screen.getByRole('textbox', { name: '訊息輸入' })).toBeDisabled()
    expect(screen.getByText('請先重試上一則訊息，再繼續輸入。')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('補上')).toBeInTheDocument()
    expect(posts).toBe(2)
    await waitFor(() => expect(screen.getByRole('textbox', { name: '訊息輸入' })).toBeEnabled())
  })

  it('switches to the read-only ended view when sending hits conversation_ended', async () => {
    mockDetail(makeDetail())
    server.use(http.post('/api/conversations/c1/messages', () => apiError(409, 'conversation_ended')))
    mockSummaries(makeSummary())
    renderPage()
    await send('新的想法')
    expect(await screen.findByText('合成立場：支持扳動拉桿')).toBeInTheDocument()
    expect(screen.getByText('這場對話已結束，只能閱讀。')).toBeInTheDocument()
    expect(screen.queryByRole('textbox', { name: '訊息輸入' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '重試' })).not.toBeInTheDocument()
  })

  it('switches to the ended view when the stream reports conversation_ended', async () => {
    mockDetail(makeDetail())
    server.use(
      http.post('/api/conversations/c1/messages', () => HttpResponse.json(sentMessage(), { status: 201 })),
      http.get('/api/conversations/c1/stream', () => sse(errorFrame('conversation_ended'))),
    )
    mockSummaries(makeSummary())
    renderPage()
    await send('新的想法')
    expect(await screen.findByText('合成立場：支持扳動拉桿')).toBeInTheDocument()
    expect(screen.queryByRole('textbox', { name: '訊息輸入' })).not.toBeInTheDocument()
  })

  it('keeps the partial text, shows the error and completes on retry after a mid-stream error', async () => {
    mockDetail(makeDetail())
    let streams = 0
    server.use(
      http.post('/api/conversations/c1/messages', () => HttpResponse.json(sentMessage(), { status: 201 })),
      http.get('/api/conversations/c1/stream', () => {
        streams += 1
        return streams === 1
          ? sse(delta('寫到一半'), errorFrame('ai_unavailable'))
          : sse(delta('完整的回覆'), doneFrame())
      }),
    )
    renderPage()
    await send('新的想法')
    expect(await screen.findByText('AI 暫時無法回覆。')).toBeInTheDocument()
    expect(screen.getByText('寫到一半')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('完整的回覆')).toBeInTheDocument()
    expect(screen.queryByText('寫到一半')).not.toBeInTheDocument()
    expect(screen.queryByText('AI 暫時無法回覆。')).not.toBeInTheDocument()
  })

  it('ignores a malformed done frame: the reply stays complete and the stage is unchanged', async () => {
    mockDetail(makeDetail({ stage: 2 }))
    server.use(
      http.post('/api/conversations/c1/messages', () => HttpResponse.json(sentMessage(), { status: 201 })),
      http.get('/api/conversations/c1/stream', () => sse(delta('完整回覆'), 'event: done\ndata: oops\n\n')),
    )
    renderPage()
    await send('新的想法')
    expect(await screen.findByText('完整回覆')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '重試' })).not.toBeInTheDocument()
    expect(within(screen.getByTestId('stage-indicator')).getByText('論證').closest('li')).toHaveAttribute(
      'aria-current',
      'step',
    )
  })

  it('announces replies: the thread is a polite log region', async () => {
    mockDetail(makeDetail())
    renderPage()
    const log = await screen.findByRole('log', { name: '對話內容' })
    expect(log).toHaveAttribute('aria-live', 'polite')
    expect(within(log).getByText('你說的「應該」是依據什麼原則呢？')).toBeInTheDocument()
  })

  it('ai_unavailable shows an error with retry; retrying only re-opens the stream (no second POST)', async () => {
    mockDetail(makeDetail())
    let posts = 0
    let streams = 0
    server.use(
      http.post('/api/conversations/c1/messages', () => {
        posts += 1
        return HttpResponse.json(sentMessage(), { status: 201 })
      }),
      http.get('/api/conversations/c1/stream', () => {
        streams += 1
        return streams === 1 ? sse(errorFrame('ai_unavailable')) : sse(delta('第二次成功'), doneFrame())
      }),
    )
    renderPage()
    await send('新的想法')
    expect(await screen.findByText('AI 暫時無法回覆。')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('第二次成功')).toBeInTheDocument()
    expect(posts).toBe(1)
    expect(streams).toBe(2)
    expect(screen.queryByText('AI 暫時無法回覆。')).not.toBeInTheDocument()
    // 學生訊息只出現一次
    expect(screen.getAllByText('新的想法')).toHaveLength(1)
  })

  it('shows the network error text when the connection drops mid-stream', async () => {
    mockDetail(makeDetail())
    server.use(
      http.post('/api/conversations/c1/messages', () => HttpResponse.json(sentMessage(), { status: 201 })),
      http.get('/api/conversations/c1/stream', () => HttpResponse.error()),
    )
    renderPage()
    await send('新的想法')
    expect(await screen.findByText('無法連線到伺服器，請檢查網路後再試。')).toBeInTheDocument()
  })

  it('offers a stream-only retry when the loaded history ends with an unanswered student message', async () => {
    mockDetail(makeDetail({}, [makeMessage({ id: 'm1', content: '還沒被回覆的想法' })]))
    let streams = 0
    server.use(
      http.get('/api/conversations/c1/stream', () => {
        streams += 1
        return sse(delta('補上的回覆'), doneFrame())
      }),
    )
    renderPage()
    expect(await screen.findByText('AI 還沒有回覆你的最後一則訊息。')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('補上的回覆')).toBeInTheDocument()
    expect(streams).toBe(1)
    await waitFor(() => expect(screen.queryByText('AI 還沒有回覆你的最後一則訊息。')).not.toBeInTheDocument())
  })
})

describe('ConversationPage: suggest end', () => {
  it('shows the wrap-up banner after a done event with suggest_end', async () => {
    mockDetail(makeDetail())
    server.use(
      http.post('/api/conversations/c1/messages', () => HttpResponse.json(sentMessage(), { status: 201 })),
      http.get('/api/conversations/c1/stream', () => sse(delta('收尾提問'), doneFrame({ suggest_end: true }))),
    )
    renderPage()
    await screen.findByRole('textbox', { name: '訊息輸入' })
    expect(screen.queryByTestId('suggest-end')).not.toBeInTheDocument()
    await send('新的想法')
    const banner = await screen.findByTestId('suggest-end')
    expect(banner).toHaveTextContent('討論得差不多了')
    expect(within(banner).getByRole('button', { name: '結束討論' })).toBeInTheDocument()
  })

  it('shows the banner immediately when the conversation is already converge_ready', async () => {
    mockDetail(makeDetail({ converge_ready: true }))
    renderPage()
    expect(await screen.findByTestId('suggest-end')).toBeInTheDocument()
  })
})

describe('ConversationPage: ending', () => {
  it('auto_ended: goes straight to the summary without opening a stream', async () => {
    mockDetail(makeDetail())
    server.use(http.post('/api/conversations/c1/messages', () => HttpResponse.json(sentMessage(true), { status: 201 })))
    // /stream 沒有 handler：若有請求會因未處理而讓測試失敗
    mockSummaries(makeSummary({ status: 'pending', stance: null, reasons: null, turning_points: null, completed_at: null }), makeSummary())
    renderPage()
    await send('最後一個想法')
    expect(await screen.findByText('合成立場：支持扳動拉桿')).toBeInTheDocument()
    expect(screen.getByText('已結束')).toBeInTheDocument()
    expect(screen.queryByRole('textbox', { name: '訊息輸入' })).not.toBeInTheDocument()
    expect(screen.getByText('最後一個想法')).toBeInTheDocument()
  })

  it('asks for confirmation, ends, polls while pending, then shows the ready summary', async () => {
    mockDetail(makeDetail())
    let ends = 0
    server.use(
      http.post('/api/conversations/c1/end', () => {
        ends += 1
        return HttpResponse.json({ status: 'pending' }, { status: 202 })
      }),
    )
    const pending = makeSummary({ status: 'pending', stance: null, reasons: null, turning_points: null, completed_at: null })
    const calls = mockSummaries(pending, pending, makeSummary())
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: '結束討論' }))
    const dialog = await screen.findByRole('alertdialog')
    expect(ends).toBe(0)
    await userEvent.click(within(dialog).getByRole('button', { name: '結束並產生總結' }))

    expect(await screen.findByText('正在產生總結…')).toBeInTheDocument()
    expect(await screen.findByText('合成立場：支持扳動拉桿')).toBeInTheDocument()
    expect(screen.getByText('合成理由：結果論')).toBeInTheDocument()
    expect(screen.getByText('合成轉折：考慮了權利')).toBeInTheDocument()
    expect(screen.getByText('立場')).toBeInTheDocument()
    expect(screen.getByText('核心理由')).toBeInTheDocument()
    expect(screen.getByText('思考轉折')).toBeInTheDocument()
    expect(ends).toBe(1)
    expect(calls.count).toBe(3)
    // 結束後唯讀，沒有編輯控制
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '結束討論' })).not.toBeInTheDocument()
    expect(screen.getByText('這場對話已結束，只能閱讀。')).toBeInTheDocument()
  })

  it('shows the translated error and stays active when ending is refused (empty conversation)', async () => {
    mockDetail(makeDetail({}, []))
    server.use(http.post('/api/conversations/c1/end', () => apiError(409, 'empty_conversation')))
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: '結束討論' }))
    const dialog = await screen.findByRole('alertdialog')
    await userEvent.click(within(dialog).getByRole('button', { name: '結束並產生總結' }))
    expect(await within(dialog).findByText('還沒有討論內容，無法結束。')).toBeInTheDocument()
    await userEvent.click(within(dialog).getByRole('button', { name: '取消' }))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
    expect(screen.getByRole('textbox', { name: '訊息輸入' })).toBeInTheDocument()
  })

  it('shows reply_in_progress when the backend refuses to end during a reply', async () => {
    mockDetail(makeDetail())
    server.use(http.post('/api/conversations/c1/end', () => apiError(409, 'reply_in_progress')))
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: '結束討論' }))
    const dialog = await screen.findByRole('alertdialog')
    await userEvent.click(within(dialog).getByRole('button', { name: '結束並產生總結' }))
    expect(await within(dialog).findByText('AI 正在回覆，請稍候。')).toBeInTheDocument()
  })

  it('does not double submit while ending is pending', async () => {
    mockDetail(makeDetail())
    let ends = 0
    let release: () => void = () => undefined
    const gate = new Promise<void>((r) => (release = r))
    server.use(
      http.post('/api/conversations/c1/end', async () => {
        ends += 1
        await gate
        return HttpResponse.json({}, { status: 202 })
      }),
    )
    mockSummaries(makeSummary())
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: '結束討論' }))
    const dialog = await screen.findByRole('alertdialog')
    const confirm = within(dialog).getByRole('button', { name: '結束並產生總結' })
    await userEvent.click(confirm)
    await waitFor(() => expect(confirm).toBeDisabled())
    // 結束中輸入框也停用（對話框開啟時背景是 aria-hidden）
    expect(screen.getByRole('textbox', { name: '訊息輸入', hidden: true })).toBeDisabled()
    await userEvent.click(confirm)
    release()
    await screen.findByText('合成立場：支持扳動拉桿')
    expect(ends).toBe(1)
  })

  it('shows the summary of an already ended conversation read-only', async () => {
    mockDetail(makeDetail({ status: 'ended', ended_at: '2026-03-05T09:00:00Z' }))
    mockSummaries(makeSummary())
    renderPage()
    expect(await screen.findByText('合成立場：支持扳動拉桿')).toBeInTheDocument()
    expect(screen.getByText('已結束')).toBeInTheDocument()
    expect(screen.getByText('我認為應該扳動拉桿。')).toBeInTheDocument()
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '送出' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '結束討論' })).not.toBeInTheDocument()
    expect(screen.getByText('這份總結由 AI 產生，你無法編輯。')).toBeInTheDocument()
  })
})

describe('ConversationPage: summary states', () => {
  const failed = makeSummary({ status: 'failed', stance: null, reasons: null, turning_points: null, completed_at: null })

  it('failed summary offers retry, which POSTs summary/retry and polls again', async () => {
    mockDetail(makeDetail({ status: 'ended' }))
    mockSummaries(failed, makeSummary())
    let retries = 0
    server.use(
      http.post('/api/conversations/c1/summary/retry', () => {
        retries += 1
        return HttpResponse.json({}, { status: 202 })
      }),
    )
    renderPage()
    expect(await screen.findByText('總結產生失敗。')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '重新產生總結' }))
    expect(await screen.findByText('合成立場：支持扳動拉桿')).toBeInTheDocument()
    expect(retries).toBe(1)
  })

  it('translates summary_not_retryable', async () => {
    mockDetail(makeDetail({ status: 'ended' }))
    mockSummaries(failed)
    server.use(http.post('/api/conversations/c1/summary/retry', () => apiError(409, 'summary_not_retryable')))
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: '重新產生總結' }))
    expect(await screen.findByText('目前無法重新產生總結。')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '重新產生總結' })).toBeEnabled()
  })

  it('stops polling after the time cap and offers to check again', async () => {
    mockDetail(makeDetail({ status: 'ended' }))
    const pending = makeSummary({ status: 'pending', stance: null, reasons: null, turning_points: null, completed_at: null })
    const calls = mockSummaries(pending)
    renderPage()
    expect(await screen.findByText('總結花的時間比預期久。')).toBeInTheDocument()
    const stopped = calls.count
    vi.useFakeTimers({ shouldAdvanceTime: true })
    await act(() => vi.advanceTimersByTimeAsync(2000))
    vi.useRealTimers()
    expect(calls.count).toBe(stopped)
    server.use(http.get('/api/conversations/c1/summary', () => HttpResponse.json(makeSummary())))
    await userEvent.click(screen.getByRole('button', { name: '再檢查一次' }))
    expect(await screen.findByText('合成立場：支持扳動拉桿')).toBeInTheDocument()
  })

  it('shows a translated error with retry when the summary request fails', async () => {
    mockDetail(makeDetail({ status: 'ended' }))
    server.use(http.get('/api/conversations/c1/summary', () => apiError(500, 'internal')))
    renderPage()
    expect(await screen.findByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
    server.use(http.get('/api/conversations/c1/summary', () => HttpResponse.json(makeSummary())))
    await userEvent.click(screen.getByRole('button', { name: '重試' }))
    expect(await screen.findByText('合成立場：支持扳動拉桿')).toBeInTheDocument()
  })

  it('stops polling when leaving the page', async () => {
    mockDetail(makeDetail({ status: 'ended' }))
    const pending = makeSummary({ status: 'pending', stance: null, reasons: null, turning_points: null, completed_at: null })
    const calls = mockSummaries(pending)
    const { unmount } = renderPage()
    await screen.findByText('正在產生總結…')
    await waitFor(() => expect(calls.count).toBeGreaterThan(1))
    unmount()
    const atUnmount = calls.count
    vi.useFakeTimers({ shouldAdvanceTime: true })
    await vi.advanceTimersByTimeAsync(2000)
    vi.useRealTimers()
    expect(calls.count).toBe(atUnmount)
  })

  it('shows the error when summary retry succeeds but the next poll fails', async () => {
    mockDetail(makeDetail({ status: 'ended' }))
    let gets = 0
    server.use(
      http.get('/api/conversations/c1/summary', () => {
        gets += 1
        return gets === 1 ? HttpResponse.json(failed) : apiError(500, 'internal')
      }),
      http.post('/api/conversations/c1/summary/retry', () => HttpResponse.json({}, { status: 202 })),
    )
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: '重新產生總結' }))
    expect(await screen.findByText('系統發生錯誤，請稍後再試。')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '重試' })).toBeInTheDocument()
  })

  it('exposes the summary as a named region', async () => {
    mockDetail(makeDetail({ status: 'ended' }))
    mockSummaries(makeSummary())
    renderPage()
    expect(await screen.findByRole('region', { name: 'AI 學習總結' })).toBeInTheDocument()
  })
})

describe('privacy', () => {
  it('does not log message text', async () => {
    const spies = (['log', 'info', 'warn', 'error', 'debug'] as const).map((m) => vi.spyOn(console, m))
    mockDetail(makeDetail())
    server.use(
      http.post('/api/conversations/c1/messages', () => HttpResponse.json(sentMessage(), { status: 201 })),
      http.get('/api/conversations/c1/stream', () => sse(delta('回覆'), doneFrame())),
    )
    renderPage()
    await send('新的想法')
    await screen.findByText('回覆')
    for (const s of spies) {
      const logged = JSON.stringify(s.mock.calls)
      expect(logged).not.toContain('新的想法')
      expect(logged).not.toContain('回覆')
    }
    spies.forEach((s) => s.mockRestore())
  })
})
