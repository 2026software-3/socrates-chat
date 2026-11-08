import { screen, within } from '@testing-library/react'
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
    renderTeacherPage(<TopicsPage />)
    expect(await screen.findByText('電車難題')).toBeInTheDocument()
    await userEvent.click(screen.getByRole('button', { name: '+ 新增題目' }))
    const dialog = await screen.findByRole('dialog', { name: '新增題目' })
    await userEvent.type(within(dialog).getByLabelText('題目名稱'), '說謊可以嗎')
    await userEvent.click(within(dialog).getByRole('button', { name: '新增題目' }))
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
    renderTeacherPage(<TopicsPage />)
    await userEvent.click(await screen.findByRole('button', { name: '停用「電車難題」' }))
    await userEvent.click(within(await screen.findByRole('alertdialog')).getByRole('button', { name: '停用題目' }))
    await screen.findByText('已停用')
    expect(url).toBe('/api/topics/t1')
  })

  it('retries pending dimensions and edits one language on a locked topic', async () => {
    const empty = { dimensions: null, locked: false, prompt_version: null, pending_languages: [] }
    const dimension = {
      id: 'd1',
      name: { zh_tw: '謹慎', en: 'Caution', es: 'Cautela' },
      description: { zh_tw: '先衡量風險', en: 'Consider risks', es: 'Considera los riesgos' },
      rubric: ['一', '二', '三', '四', '五'],
    }
    const ready = {
      dimensions: Array.from({ length: 6 }, (_, index) => ({ ...dimension, id: `d${index + 1}` })),
      locked: true, prompt_version: 'topic-dimensions-v1', pending_languages: [],
    }
    let submitted: unknown
    server.use(
      http.get('/api/topics', () => HttpResponse.json([trolley])),
      http.get('/api/topics/t1/dimensions', () => HttpResponse.json(empty)),
      http.post('/api/topics/t1/dimensions/retry', () => HttpResponse.json(ready)),
      http.patch('/api/topics/t1/dimensions', async ({ request }) => {
        submitted = await request.json()
        return HttpResponse.json(ready)
      }),
    )
    renderTeacherPage(<TopicsPage />)
    const user = userEvent.setup()
    await user.click(await screen.findByRole('button', { name: '雷達維度' }))
    const dialog = await screen.findByRole('dialog')
    expect(await within(dialog).findByText('維度尚未完成。題目仍可用於對話。')).toBeInTheDocument()
    await user.click(within(dialog).getByRole('button', { name: '重新產生維度' }))
    expect(await within(dialog).findByText('已有對話，評分準則已鎖定。')).toBeInTheDocument()
    await user.selectOptions(within(dialog).getByLabelText('編輯語言'), 'en')
    const name = within(dialog).getAllByLabelText('維度名稱')[0]
    expect(name).toHaveValue('Caution')
    await user.clear(name)
    await user.type(name, 'Careful judgment')
    await user.click(within(dialog).getByRole('button', { name: '儲存' }))
    expect(await within(dialog).findByText('已儲存維度。')).toBeInTheDocument()
    expect(submitted).toEqual({
      language: 'en',
      dimensions: ready.dimensions.map(({ id, name, description }) => ({
        id, name: id === 'd1' ? 'Careful judgment' : name.en, description: description.en,
      })),
    })
  })

  it('shows a recoverable error when dimension loading fails', async () => {
    let attempts = 0
    server.use(
      http.get('/api/topics', () => HttpResponse.json([trolley])),
      http.get('/api/topics/t1/dimensions', () => {
        attempts += 1
        return attempts === 1
          ? HttpResponse.json({ error: { code: 'internal' } }, { status: 500 })
          : HttpResponse.json({ dimensions: null, locked: false, pending_languages: [] })
      }),
    )
    renderTeacherPage(<TopicsPage />)
    await userEvent.click(await screen.findByRole('button', { name: '雷達維度' }))
    const dialog = await screen.findByRole('dialog')
    await userEvent.click(await within(dialog).findByRole('button', { name: '重試' }))
    expect(await within(dialog).findByText('維度尚未完成。題目仍可用於對話。')).toBeInTheDocument()
  })
})
