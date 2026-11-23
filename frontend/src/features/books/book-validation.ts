import type { BookInput } from './types'

export const textLimits: Partial<Record<keyof BookInput, number>> = { title: 255, language: 35, publication_year: 4, authors: 1000, source_url: 8192, description: 5000, recommendation_note: 2000 }
export function safeBookUrl(value: string | null): string | undefined {
  if (!value) return undefined
  try { const url = new URL(value); return ['http:', 'https:'].includes(url.protocol) && url.hostname ? value : undefined } catch { return undefined }
}
export function validIsbn(value: string): boolean {
  const isbn = value.replace(/[\s-]/g, '').toUpperCase()
  if (!isbn) return true
  if (/^\d{9}[\dX]$/.test(isbn)) return [...isbn].reduce((sum, c, i) => sum + (c === 'X' ? 10 : Number(c)) * (10 - i), 0) % 11 === 0
  return /^(978|979)\d{10}$/.test(isbn) && [...isbn].reduce((sum, c, i) => sum + Number(c) * (i % 2 ? 3 : 1), 0) % 10 === 0
}

export function validBookLanguage(value: string): boolean { return !value.trim() || /^[A-Za-z0-9_-]{1,35}$/.test(value.trim()) }
export function validPublicationYear(value: string): boolean { return !value.trim() || (/^[0-9]{4}$/.test(value.trim()) && value.trim() !== '0000') }
