import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { Route, Routes } from 'react-router'
import { describe, expect, it } from 'vitest'
import { ConversationPage } from '@/features/chat/conversation-page'
import { DEMO_REPLIES, DEMO_TOPIC_ID } from '@/features/chat/demo-replies-data'
import { makeDetail } from '@/features/chat/test-data'
import { makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

function renderDemoPage(topicId: string | null, demoMode: boolean) {
  if (demoMode) server.use(http.get('/api/demo/accounts', () => HttpResponse.json({ accounts: [] })))
  server.use(http.get('/api/conversations/c1', () => HttpResponse.json(makeDetail({ topic_id: topicId }, []))))
  mockMe(makeMe({ student: true }))
  return renderApp(
    <Routes>
      <Route path="/conversations/:id" element={<ConversationPage />} />
    </Routes>,
    { route: '/conversations/c1' },
  )
}

describe('DemoReplies', () => {
  it('fills the composer with the next reply without sending', async () => {
    renderDemoPage(DEMO_TOPIC_ID, true)
    await userEvent.click(await screen.findByRole('button', { name: new RegExp(`下一則（第 1 則）：.*${DEMO_REPLIES[0]}`) }))
    expect(screen.getByRole('textbox', { name: '訊息輸入' })).toHaveValue(DEMO_REPLIES[0])
    expect(screen.getByText('回合：0')).toBeInTheDocument()
  })

  it('is hidden when the backend is not in demo mode', async () => {
    renderDemoPage(DEMO_TOPIC_ID, false)
    await screen.findByRole('textbox', { name: '訊息輸入' })
    expect(screen.queryByText('示範模式：快速回覆')).not.toBeInTheDocument()
  })

  it('is hidden for topics other than the live-demo one', async () => {
    renderDemoPage('t1', true)
    await screen.findByRole('textbox', { name: '訊息輸入' })
    expect(screen.queryByText('示範模式：快速回覆')).not.toBeInTheDocument()
  })
})
