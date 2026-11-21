import { act, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { delay, http, HttpResponse } from 'msw'
import { Route, Routes } from 'react-router'
import { describe, expect, it } from 'vitest'
import { ConversationPage } from '@/features/chat/conversation-page'
import { makeDetail, makeSummary } from '@/features/chat/test-data'
import { apiError, makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

const root = { id: 'TREE-C0', kind: 'claim', label: '支持使用工具', source_round: 1, evidence: '我支持。', parent: null, relation: null, origin: 'user', children: [
  { id: 'TREE-P1', kind: 'premise', label: '<img src=x onerror=alert(1)>節省時間', source_round: 1, evidence: '可以省時。', parent: 'TREE-C0', relation: 'support', origin: 'user', children: [] },
] }
const ready = { status: 'ready', tree: { topic_id: 'TREE', title: '合成題目', node_count: 2, review_status: 'unreviewed', root }, prompt_version: 'v2.1.1', completed_at: '2026-11-10T00:00:00Z' }
const state = (status: string) => ({ status, tree: null, prompt_version: null, completed_at: null })

function renderPage(ended = true) {
  mockMe(makeMe({ student: true }))
  server.use(
    http.get('/api/conversations/c1', () => HttpResponse.json(makeDetail({ status: ended ? 'ended' : 'active' }))),
    http.get('/api/conversations/c1/summary', () => HttpResponse.json(makeSummary())),
  )
  return renderApp(<Routes><Route path="/conversations/:id" element={<ConversationPage />} /></Routes>, { route: '/conversations/c1' })
}

describe('saved argument tree button', () => {
  it('generates only after a click, polls, and renders an escaped tree and optional JSON', async () => {
    let reads = 0
    let posts = 0
    server.use(
      http.get('/api/conversations/c1/argument-tree', () => HttpResponse.json(reads++ === 0 ? state('missing') : ready)),
      http.post('/api/conversations/c1/argument-tree', async () => { posts++; await delay(30); return HttpResponse.json(state('pending'), { status: 202 }) }),
    )
    renderPage()
    const button = await screen.findByRole('button', { name: '總結論證樹' })
    expect(reads).toBe(0)
    expect(posts).toBe(0)
    await userEvent.click(button)
    expect(await screen.findByText('正在產生論證樹…')).toBeInTheDocument()
    expect(await screen.findByText('支持使用工具', {}, { timeout: 5000 })).toBeInTheDocument()
    expect(posts).toBe(1)
    const dialog = screen.getByRole('dialog')
    expect(within(dialog).getByText('<img src=x onerror=alert(1)>節省時間')).toBeInTheDocument()
    expect(dialog.querySelector('img')).toBeNull()
    await userEvent.click(within(dialog).getByText('檢視 JSON'))
    expect(within(dialog).getByTestId('argument-tree-json')).toHaveTextContent('"node_count": 2')
  })

  it('reads a saved tree after remount without another model request', async () => {
    let posts = 0
    server.use(
      http.get('/api/conversations/c1/argument-tree', () => HttpResponse.json(ready)),
      http.post('/api/conversations/c1/argument-tree', () => { posts++; return HttpResponse.json(state('pending')) }),
    )
    const first = renderPage()
    await userEvent.click(await screen.findByRole('button', { name: '總結論證樹' }))
    await screen.findByText('支持使用工具')
    first.unmount()
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: '總結論證樹' }))
    await screen.findByText('支持使用工具')
    expect(posts).toBe(0)
  })

  it('offers explicit retry for a failed saved attempt', async () => {
    let posts = 0
    server.use(
      http.get('/api/conversations/c1/argument-tree', () => HttpResponse.json(state('failed'))),
      http.post('/api/conversations/c1/argument-tree', () => { posts++; return HttpResponse.json(ready) }),
    )
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: '總結論證樹' }))
    expect(await screen.findByText('論證樹產生失敗，請重試。')).toBeInTheDocument()
    expect(posts).toBe(0)
    await userEvent.click(screen.getByRole('button', { name: '重試產生論證樹' }))
    await screen.findByText('支持使用工具')
    expect(posts).toBe(1)
  })

  it('shows API quota errors and blocks duplicate clicks while starting', async () => {
    let posts = 0
    server.use(
      http.get('/api/conversations/c1/argument-tree', async () => { await delay(30); return HttpResponse.json(state('missing')) }),
      http.post('/api/conversations/c1/argument-tree', () => { posts++; return apiError(429, 'ai_usage_exceeded') }),
    )
    renderPage()
    await userEvent.dblClick(await screen.findByRole('button', { name: '總結論證樹' }))
    await waitFor(() => expect(posts).toBe(1))
    expect(await screen.findByRole('alert')).toHaveTextContent('AI')
  })

  it('does not offer generation during an active conversation', async () => {
    renderPage(false)
    await screen.findByRole('textbox', { name: '訊息輸入' })
    expect(screen.queryByRole('button', { name: '總結論證樹' })).not.toBeInTheDocument()
  })

  it('stops polling when the dialog is closed', async () => {
    let reads = 0
    server.use(http.get('/api/conversations/c1/argument-tree', () => { reads++; return HttpResponse.json(state('pending')) }))
    renderPage()
    await userEvent.click(await screen.findByRole('button', { name: '總結論證樹' }))
    await screen.findByText('正在產生論證樹…')
    await userEvent.keyboard('{Escape}')
    const before = reads
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 2200)) })
    expect(reads).toBe(before)
  })
})
