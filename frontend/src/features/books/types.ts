export type BookSource = { source: string; source_id: string }

export type BookInput = {
  external_sources: BookSource[]
  title: string
  authors: string | null
  language: string | null
  publication_year: string | null
  isbn: string | null
  source_url: string | null
  description: string | null
  recommendation_note: string | null
}

export type Book = BookInput & {
  id: string
  work_id: string
  created_by: string | null
  is_active: boolean
  created_at: string
  updated_at: string
}

export type BookCandidate = Omit<BookInput, 'recommendation_note' | 'external_sources'> & { source: string; source_id: string }
export type DeletionImpact = { deletes_work?: boolean; topic_count: number; activity_topic_count: number }

export type BookWork = { id: string; title: string; is_active: boolean; created_at: string; updated_at: string; editions: Book[] }
export type WorkImpact = DeletionImpact & { edition_count: number }
