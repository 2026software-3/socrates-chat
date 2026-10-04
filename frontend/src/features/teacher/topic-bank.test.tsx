import { screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { TopicsPage } from '@/features/admin/topics-page'
import { renderTeacherPage } from '@/features/teacher/test-helpers'
import type { Topic } from '@/lib/types'
import { server } from '@/test/server'

const trolley: Topic = {
  id: 't1',
  title: '電車難題',
  description: '你會拉桿嗎？',
  category: '倫理學',
  is_active: true,
}

describe('teacher topic bank', () => {
  it('lets a teacher see the built-in trolley problem and add a topic through /api/topics', async () => {
    const topics = [trolley]
    let body: unknown
    server.use(
      http.get('/api/topics', () => HttpResponse.json(topics)),
      http.post('/api/topics', async ({ request }) => {
        body = await request.json()
        const created = { ...trolley, id: 't2', title: (body as { title: string }).title }
        topics.push(created)
        return HttpResponse.json(created, { status: 201 })
      }),
    )
    renderTeacherPage(<TopicsPage base="/api/topics" />)
    expect(await screen.findByText('電車難題')).toBeInTheDocument()
    await userEvent.type(screen.getByLabelText('標題'), '說謊可以嗎')
    await userEvent.click(screen.getByRole('button', { name: '新增題目' }))
    expect(await screen.findByText('說謊可以嗎')).toBeInTheDocument()
    expect(body).toEqual({ title: '說謊可以嗎' })
  })

  it('toggles a topic through the teacher endpoint', async () => {
    let url = ''
    server.use(
      http.get('/api/topics', () => HttpResponse.json([trolley])),
      http.patch('/api/topics/:id', async ({ request }) => {
        url = new URL(request.url).pathname
        return HttpResponse.json({ ...trolley, ...((await request.json()) as object) })
      }),
    )
    renderTeacherPage(<TopicsPage base="/api/topics" />)
    await userEvent.click(await screen.findByRole('switch', { name: '啟用「電車難題」' }))
    await screen.findByText('已停用')
    expect(url).toBe('/api/topics/t1')
  })
})
