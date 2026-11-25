import type { Book, BookWork } from './types'

export function normalizeBookLanguage(language: string | null | undefined): string | null {
  const text = language?.trim()
  if (!text) return null
  return ({ '中文': 'zh', '繁體中文': 'zh-Hant', '简体中文': 'zh-Hans', '簡體中文': 'zh-Hans', chinese: 'zh', chi: 'zh', zho: 'zh', '英文': 'en', '英語': 'en', english: 'en', eng: 'en', '西文': 'es', '西班牙文': 'es', '西班牙語': 'es', spanish: 'es', español: 'es', spa: 'es', '其他': 'other' } as Record<string, string>)[text.toLowerCase()] ?? text
}
export function bookLanguage(language: string | null) {
  const code = normalizeBookLanguage(language)?.toLowerCase().split(/[-_]/)[0] || 'missing'
  return ({ eng: 'en', spa: 'es', zho: 'zh', chi: 'zh' } as Record<string, string>)[code] ?? code
}
export function filterBooks(books: Book[], query: string, language: string, sort = 'newest') {
  return books.filter((book) => (language === 'all' || bookLanguage(book.language) === language) &&
    `${book.title} ${book.authors ?? ''}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()))
    .sort((a, b) => {
      const title = a.title.localeCompare(b.title)
      if (sort === 'title') return title
      const time = (Date.parse(b.created_at) || 0) - (Date.parse(a.created_at) || 0)
      return (sort === 'oldest' ? -time : time) || title
    })
}

export function filterWorks(works: BookWork[], query: string, language: string, sort = 'newest') {
  const search = query.trim().toLocaleLowerCase()
  return works.filter((work) => (language === 'all' || work.editions.some((edition) => bookLanguage(edition.language) === language)) &&
    [work.title, ...work.editions.flatMap((edition) => [edition.title, edition.authors ?? ''])].join(' ').toLocaleLowerCase().includes(search))
    .sort((a, b) => sort === 'title' ? a.title.localeCompare(b.title) : ((Date.parse(b.created_at) || 0) - (Date.parse(a.created_at) || 0)) * (sort === 'oldest' ? -1 : 1) || a.title.localeCompare(b.title))
}
