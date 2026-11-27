import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'
import { BookForm } from './book-form'
import { bookLanguage } from './book-filtering'
import type { BookInput } from './types'

const initial: BookInput = { title: '合成書名', authors: null, language: null, publication_year: null, isbn: null, source_url: null, description: null, recommendation_note: null, external_sources: [] }

describe('book language selection', () => {
  beforeEach(() => mockMe(makeMe({ teacher: true })))
  it.each([['中文', 'zh'], ['英文', 'en'], ['西班牙文', 'es'], ['其他', 'other']])('saves %s as a stable value', async (name, code) => {
    const save = vi.fn().mockResolvedValue(undefined)
    renderApp(<BookForm initial={initial} save={save} cancel={() => {}} />)
    await userEvent.selectOptions(screen.getByRole('combobox', { name: '語言（選填）' }), screen.getByRole('option', { name }))
    await userEvent.click(screen.getByRole('button', { name: '儲存書目' }))
    await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ language: code })))
  })
  it.each(['zh-TW', 'zh-CN', 'fr'])('preserves specific imported language %s', async (language) => {
    const save = vi.fn().mockResolvedValue(undefined)
    server.use(http.get('/api/books/search', () => HttpResponse.json([{ source: 'Google Books', source_id: 'synthetic', ...initial, language }])))
    renderApp(<BookForm initial={initial} save={save} cancel={() => {}} />)
    await userEvent.click(screen.getByRole('button', { name: '依書名搜尋補資料' }))
    await waitFor(() => expect(screen.getByRole('combobox', { name: '語言（選填）' })).toHaveValue(language.startsWith('zh') ? 'zh' : 'other'))
    await userEvent.click(screen.getByRole('button', { name: '儲存書目' }))
    await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ language })))
  })
  it('allows an existing language to be cleared', async () => {
    const save = vi.fn().mockResolvedValue(undefined)
    renderApp(<BookForm initial={{ ...initial, language: 'en' }} save={save} cancel={() => {}} />)
    await userEvent.selectOptions(screen.getByRole('combobox', { name: '語言（選填）' }), '')
    await userEvent.click(screen.getByRole('button', { name: '儲存書目' }))
    await waitFor(() => expect(save).toHaveBeenCalledWith(expect.objectContaining({ language: null })))
  })
  it.each([['中文', 'zh'], ['英文', 'en'], ['西文', 'es'], ['English', 'en'], ['西班牙文', 'es']])('recognizes legacy language label %s when filtering', (value, expected) => {
    expect(bookLanguage(value)).toBe(expected)
  })
})
