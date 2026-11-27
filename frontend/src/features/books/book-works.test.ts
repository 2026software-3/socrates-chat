import { expect, it } from 'vitest'
import { filterWorks } from './book-filtering'
import type { BookWork } from './types'
it('searches all edition metadata and filters languages without splitting works', () => {
 const work = { id: 'work', title: 'Work', created_at: '', editions: [{ title: 'Translation', authors: 'Translator', language: 'es' }, { title: 'Original', authors: null, language: 'en' }] } as BookWork
 expect(filterWorks([work], 'translator', 'en')).toEqual([work])
 expect(filterWorks([work], '', 'zh')).toEqual([])
 expect(filterWorks([work], 'translation', 'all')).toHaveLength(1)
})
