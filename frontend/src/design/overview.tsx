import { useRef, useState } from 'react'
import { Calendar, Pencil, Plus, User } from 'lucide-react'
import { Alert } from '@/components/vedute/alert'
import { Button } from '@/components/vedute/button'
import { Field } from '@/components/vedute/field'
import { Meta } from '@/components/vedute/meta'
import { Pagination } from '@/components/vedute/pagination'
import { Select } from '@/components/vedute/select'
import { Skeleton, SkeletonRegion } from '@/components/vedute/skeleton'
import { StatusChip } from '@/components/vedute/status-chip'
import { Switch } from '@/components/vedute/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/vedute/tabs'
import { Textarea } from '@/components/vedute/textarea'
import { Toast, type ToastNotification } from '@/components/vedute/toast'

const pageLabels = {
  navigation: '教室頁碼',
  previous: '上一頁',
  next: '下一頁',
  page: (page: number) => `第 ${page} 頁`,
}

const classrooms = [
  { value: 'a', label: '倫理學討論室' },
  { value: 'b', label: '知識論討論室' },
  { value: 'c', label: '形上學討論室', disabled: true, hint: '已封存' },
]

export function DesignOverview() {
  const [page, setPage] = useState(1)
  const [notification, setNotification] = useState<ToastNotification | null>(null)
  const notificationId = useRef(0)

  function notify(kind: ToastNotification['kind']) {
    setNotification({
      id: ++notificationId.current,
      kind,
      message: {
        success: '討論活動已儲存。',
        info: '教室連結已複製。',
        error: '儲存失敗，請重試。',
      }[kind],
      action: kind === 'error' ? { label: '重試', onClick: () => notify('success') } : undefined,
    })
  }

  return (
    <main className="mx-auto grid max-w-4xl gap-6 px-4 pt-6 pb-36 md:px-6" lang="zh-Hant">
      <h1 className="type-h1">Vedute 基礎元件</h1>

      <section className="grid gap-3 border-t border-ink-faint pt-3" aria-labelledby="buttons">
        <h2 id="buttons" className="type-h3">按鈕</h2>
        <div className="flex flex-wrap items-center gap-2">
          <Button><Plus aria-hidden="true" />建立活動</Button>
          <Button variant="quiet">取消</Button>
          <Button variant="ghost">上一步</Button>
          <Button variant="link" asChild><a href="#notifications">操作通知說明</a></Button>
          <Button variant="icon" aria-label="編輯"><Pencil aria-hidden="true" /></Button>
          <Button disabled>儲存中…</Button>
        </div>
      </section>

      <div className="grid gap-6 md:grid-cols-2">
        <section className="grid content-start gap-3 border-t border-ink-faint pt-3" aria-labelledby="switches">
          <h2 id="switches" className="type-h3">開關</h2>
          <div className="grid gap-1">
            <Switch label="活動通知" />
            <Switch label="共用書單" defaultChecked />
            <Switch label="每日摘要" disabled />
            <Switch label="活動提醒" defaultChecked disabled />
            <Switch label="公開反思" locked />
          </div>
        </section>

        <section className="grid gap-3 border-t border-ink-faint pt-3" aria-labelledby="fields">
          <h2 id="fields" className="type-h3">表單</h2>
          <Field label="活動名稱" placeholder="輸入活動名稱" error="請填寫活動名稱。" />
          <Select label="教室" placeholder="選擇教室" options={classrooms} />
          <Textarea label="討論說明" placeholder="輸入討論說明" rows={2} />
        </section>
      </div>

      <div className="grid gap-6 md:grid-cols-2">
        <section className="grid content-start gap-3 border-t border-ink-faint pt-3" aria-labelledby="tab-section">
          <h2 id="tab-section" className="type-h3">頁籤</h2>
          <Tabs defaultValue="active">
            <TabsList aria-label="篩選活動">
              <TabsTrigger value="active">進行中</TabsTrigger>
              <TabsTrigger value="archived">已封存</TabsTrigger>
            </TabsList>
            <TabsContent value="active">自由與責任</TabsContent>
            <TabsContent value="archived">沒有已封存的活動。</TabsContent>
          </Tabs>
        </section>

        <section className="grid content-start gap-3 border-t border-ink-faint pt-3" aria-labelledby="details">
          <h2 id="details" className="type-h3">狀態與資訊</h2>
          <div className="flex flex-wrap gap-2">
            <StatusChip tone="active">進行中</StatusChip>
            <StatusChip tone="draft">草稿</StatusChip>
            <StatusChip tone="inactive">已封存</StatusChip>
          </div>
          <div className="flex flex-wrap gap-4">
            <Meta icon={User} label="作者" value="林同學" />
            <Meta icon={Calendar} label="建立日期" value="2026/10/14" />
          </div>
        </section>
      </div>

      <div className="grid gap-6 md:grid-cols-2">
        <section className="grid content-start gap-3 border-t border-ink-faint pt-3" aria-labelledby="alerts">
          <h2 id="alerts" className="type-h3">提示</h2>
          <Alert>本活動已封存，目前僅供瀏覽。</Alert>
          <Alert kind="error">無法載入學生名單。</Alert>
        </section>

        <section className="grid content-start gap-3 border-t border-ink-faint pt-3" aria-labelledby="loading">
          <h2 id="loading" className="type-h3">載入中</h2>
          <SkeletonRegion loadingLabel="正在載入教室" className="grid gap-2">
            <Skeleton className="h-5 w-full" />
            <Skeleton className="h-5 w-3/5" />
          </SkeletonRegion>
        </section>
      </div>

      <section className="grid gap-3 border-t border-ink-faint pt-3" aria-labelledby="pagination">
        <h2 id="pagination" className="type-h3">分頁</h2>
        <Pagination page={page} pageCount={5} labels={pageLabels} onPageChange={setPage} />
      </section>

      <section className="grid gap-3 border-t border-ink-faint pt-3" aria-labelledby="notifications">
        <h2 id="notifications" className="type-h3">操作通知</h2>
        <div className="flex flex-wrap gap-2">
          <Button variant="quiet" onClick={() => notify('success')}>儲存活動</Button>
          <Button variant="quiet" onClick={() => notify('info')}>複製連結</Button>
          <Button variant="quiet" onClick={() => notify('error')}>顯示儲存失敗</Button>
        </div>
        <Toast notification={notification} dismissLabel="關閉通知"
          onDismiss={(id) => setNotification((current) => current?.id === id ? null : current)} />
      </section>
    </main>
  )
}
