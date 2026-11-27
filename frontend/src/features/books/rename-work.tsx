import { useId, useState, type FormEvent } from 'react'
import { ErrorBlock } from '@/components/page'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { useT } from '@/i18n'
import { api } from '@/lib/api'
import type { BookWork } from './types'
export function RenameWork({ work, close, changed }: { work: BookWork; close: () => void; changed: () => void }) {
 const t=useT();const id=useId();const [title,setTitle]=useState(work.title);const [pending,setPending]=useState(false);const [error,setError]=useState<unknown>()
 const valid=title.trim().length>0 && [...title.trim()].length<=255
 async function save(event:FormEvent) {
  event.preventDefault();if(pending || !valid)return;setPending(true);setError(undefined)
  try{await api.patch(`/api/book-works/${encodeURIComponent(work.id)}`,{title:title.trim()});changed();close()}catch(err){setError(err)}finally{setPending(false)}
 }
 return <Dialog open onOpenChange={(open)=>!open&&!pending&&close()}><DialogContent showCloseButton={false}><DialogHeader><DialogTitle>{t('books.renameWork')}</DialogTitle><DialogDescription>{t('books.renameWorkHint')}</DialogDescription></DialogHeader><form onSubmit={(event)=>void save(event)} className="space-y-3"><Label htmlFor={id}>{t('books.workTitle')}</Label><Input id={id} value={title} disabled={pending} aria-invalid={!valid} onChange={(event)=>setTitle(event.target.value)} />{!valid?<p role="alert">{t('error.invalid_book_title')}</p>:null}{error?<ErrorBlock error={error}/>:null}<Button type="submit" disabled={!valid||pending}>{t(pending?'books.saving':'books.saveWorkTitle')}</Button><Button type="button" variant="outline" disabled={pending} onClick={close}>{t('common.cancel')}</Button></form></DialogContent></Dialog>
}
