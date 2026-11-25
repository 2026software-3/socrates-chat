import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { beforeEach, expect, it, vi } from 'vitest'
import { makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'
import { BookForm } from './book-form'
import type { BookInput, BookWork } from './types'
const input:BookInput={title:'Synthetic',authors:null,language:null,publication_year:null,isbn:null,source_url:null,description:null,recommendation_note:null,external_sources:[]}
beforeEach(()=>mockMe(makeMe({teacher:true})))
it.each(['0000','123','2020-01-01','約2020年'])('rejects invalid year %s without saving',async(year)=>{
 const save=vi.fn();renderApp(<BookForm initial={{...input,publication_year:year}} save={save} cancel={()=>{}} />)
 await userEvent.click(screen.getByRole('button',{name:'儲存書目'}));expect(await screen.findByRole('alert')).toBeInTheDocument();expect(save).not.toHaveBeenCalled()
})
it('does not adopt invalid external year or excessive language',async()=>{
 server.use(http.get('/api/books/search',()=>HttpResponse.json([{...input,source:'Google Books',source_id:'synthetic',publication_year:'0000',language:'x'.repeat(36)}])))
 const save=vi.fn().mockResolvedValue(undefined);renderApp(<BookForm initial={input} save={save} cancel={()=>{}} />)
 await userEvent.click(screen.getByRole('button',{name:'依書名搜尋補資料'}));await screen.findByText(/部分外部欄位/)
 await userEvent.click(screen.getByRole('button',{name:'儲存書目'}));await waitFor(()=>expect(save).toHaveBeenCalledWith(expect.objectContaining({publication_year:null,language:null})))
})
it('allows a teacher to rename a shared work after correcting a failed save',async()=>{
 const work:BookWork={id:'work',title:'Old work',is_active:true,created_at:'',updated_at:'',editions:[]};let body:unknown
 server.use(http.patch('/api/book-works/work',async({request})=>{body=await request.json();return HttpResponse.json({error:{code:'internal',request_id:'synthetic'}},{status:500})}))
 server.use(http.get('/api/book-works',()=>HttpResponse.json([work])))
 renderApp(null,{route:'/teacher/books'})
 await userEvent.click(await screen.findByRole('button',{name:'修改作品名稱'}))
 const field=screen.getByLabelText('作品名稱');await userEvent.clear(field);await userEvent.type(field,'New work')
 await userEvent.click(screen.getByRole('button',{name:'儲存作品名稱'}));await screen.findByRole('alert');expect(field).toHaveValue('New work')
 server.use(http.patch('/api/book-works/work',()=>HttpResponse.json({...work,title:'New work'})))
 await userEvent.click(screen.getByRole('button',{name:'儲存作品名稱'}));await waitFor(()=>expect(screen.queryByRole('dialog')).not.toBeInTheDocument());expect(body).toEqual({title:'New work'})
})
