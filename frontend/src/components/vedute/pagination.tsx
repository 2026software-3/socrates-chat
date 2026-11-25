import { ChevronLeft, ChevronRight } from 'lucide-react'
import { Button } from './button'

type PaginationProps = {
  page: number
  pageCount: number
  onPageChange: (page: number) => void
  labels: { navigation: string; previous: string; next: string; page: (page: number) => string }
}

const MAX_VISIBLE_PAGES = 5

export function Pagination({ page, pageCount, onPageChange, labels }: PaginationProps) {
  if (pageCount < 2) return null

  const currentPage = Math.min(Math.max(page, 1), pageCount)
  const visibleCount = Math.min(pageCount, MAX_VISIBLE_PAGES)
  const firstPage = Math.min(
    Math.max(currentPage - Math.floor(visibleCount / 2), 1),
    pageCount - visibleCount + 1,
  )

  return (
    <nav aria-label={labels.navigation} className="flex flex-wrap items-center gap-1 sm:gap-1.5">
      <Button variant="icon" className="vedute-page" aria-label={labels.previous}
        disabled={currentPage === 1} onClick={() => onPageChange(currentPage - 1)}>
        <ChevronLeft className="size-4" aria-hidden="true" />
      </Button>
      {Array.from({ length: visibleCount }, (_, index) => {
        const number = firstPage + index
        const current = number === currentPage
        return (
          <Button key={number} variant="ghost" className="vedute-page" aria-label={labels.page(number)}
            aria-current={current ? 'page' : undefined} onClick={current ? undefined : () => onPageChange(number)}>
            {number}
          </Button>
        )
      })}
      <Button variant="icon" className="vedute-page" aria-label={labels.next}
        disabled={currentPage === pageCount} onClick={() => onPageChange(currentPage + 1)}>
        <ChevronRight className="size-4" aria-hidden="true" />
      </Button>
    </nav>
  )
}
