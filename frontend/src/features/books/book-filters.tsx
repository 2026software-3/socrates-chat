import { useId } from 'react'
import { Label } from '@/components/ui/label'
import { useT } from '@/i18n'
import { bookLanguage } from './book-filtering'
import type { Book } from './types'

export function BookLanguageFilter({ books, value, change }: { books: Book[]; value: string; change: (value: string) => void }) {
  const t = useT()
  const id = useId()
  const languages = [...new Set([...books.map((book) => bookLanguage(book.language)), ...(value === 'all' ? [] : [value])])].sort()
  return <div className="space-y-1.5"><Label htmlFor={id}>{t('books.filterLanguage')}</Label>
    <select id={id} className="w-full rounded border p-2" value={value} onChange={(event) => change(event.target.value)}>
      <option value="all">{t('books.allLanguages')}</option>
      {languages.map((language) => <option key={language} value={language}>{language === 'missing' ? t('books.languageMissing') : language === 'zh' ? t('books.language.zh') : language === 'en' ? t('books.language.en') : language === 'es' ? t('books.language.es') : language}</option>)}
    </select></div>
}
