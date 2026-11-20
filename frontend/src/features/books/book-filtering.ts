import type { Book, BookWork } from './types'

export function bookLanguage(language: string | null) {
  const code = language?.trim().toLowerCase().split(/[-_]/)[0] || 'missing'
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
