import { useRef, useState } from 'react'
import {
  Archive,
  ArrowRight,
  BookOpen,
  Calendar,
  CalendarClock,
  Clock,
  Ellipsis,
  Eye,
  FileText,
  Languages,
  ListChecks,
  MessageSquare,
  Network,
  Pencil,
  Play,
  Plus,
  School,
  Search,
  Send,
  User,
  Users,
} from 'lucide-react'
import { Alert } from '@/components/vedute/alert'
import { BookCover, BookCoverSkeleton } from '@/components/vedute/book-cover'
import { Button } from '@/components/vedute/button'
import { DirectoryRow, DirectoryRowSkeleton } from '@/components/vedute/directory-row'
import { EditionRow } from '@/components/vedute/edition-row'
import { Field } from '@/components/vedute/field'
import { Meta } from '@/components/vedute/meta'
import { Pagination } from '@/components/vedute/pagination'
import { PlateCard, PlateCardSkeleton } from '@/components/vedute/plate-card'
import { Select } from '@/components/vedute/select'
import { Skeleton, SkeletonRegion } from '@/components/vedute/skeleton'
import { StackBar, StackBarSkeleton } from '@/components/vedute/stack-bar'
import { StageScale, StageScaleSkeleton } from '@/components/vedute/stage-scale'
import { Stamp, StampSkeleton } from '@/components/vedute/stamp'
import { StatusChip } from '@/components/vedute/status-chip'
import { Switch } from '@/components/vedute/switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/vedute/tabs'
import { Textarea } from '@/components/vedute/textarea'
import { Toast, type ToastNotification } from '@/components/vedute/toast'
import { WorkRow, WorkRowSkeleton } from '@/components/vedute/work-row'

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

const stageLabels = ['釐清', '論證', '挑戰'] as const

const argumentSegments = [
  { label: '拉桿救五人', value: 14 },
  { label: '不介入', value: 8 },
  { label: '視情況', value: 4 },
  { label: '未表態', value: 2 },
]

export function DesignOverview() {
  const [page, setPage] = useState(1)
  const [plateCardLoaded, setPlateCardLoaded] = useState(false)
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

      <h2 className="type-h1 border-t border-ink pt-6">Vedute 版畫元件</h2>

      <section className="grid gap-3 border-t border-ink-faint pt-3" aria-labelledby="stamps">
        <h3 id="stamps" className="type-h3">版號印章</h3>
        <div className="flex flex-wrap items-center gap-3">
          <Stamp prefix="TAV." numeral={12} tone="aerugo" />
          <Stamp prefix="ACTVM" numeral={2} tone="ochra" />
          <Stamp prefix="QVAEST." numeral={1} tone="caeruleum" />
          <Stamp prefix="TAV." numeral={10} tone="paper" />
          <Stamp prefix="QVAEST." numeral={3} tone="caeruleum" size="sm" />
          <StampSkeleton />
        </div>
      </section>

      <section className="grid gap-3 border-t border-ink-faint pt-3" aria-labelledby="stage-scales">
        <h3 id="stage-scales" className="type-h3">階段刻度</h3>
        <div className="grid gap-4 md:grid-cols-2">
          <div className="grid content-start gap-3">
            <StageScale round={3} roundLabel="3 回合" labels={stageLabels} ariaLabel="釐清階段進度" />
            <StageScale round={8} roundLabel="8 回合" labels={stageLabels} ariaLabel="論證階段進度" />
            <StageScale round={15} roundLabel="15 回合" labels={stageLabels} ariaLabel="挑戰階段進度" />
          </div>
          <div className="grid content-start gap-3">
            <StageScale variant="compact" round={3} roundLabel="回合 3" labels={stageLabels} ariaLabel="精簡階段進度" />
            <StageScale variant="compact" round={9} roundLabel="回合 9" labels={stageLabels} ariaLabel="精簡論證進度" />
            <SkeletonRegion loadingLabel="正在載入階段刻度" className="grid gap-2">
              <StageScaleSkeleton showRound />
              <StageScaleSkeleton variant="compact" showRound />
            </SkeletonRegion>
          </div>
        </div>
      </section>

      <section className="grid gap-3 border-t border-ink-faint pt-3" aria-labelledby="directory-rows">
        <h3 id="directory-rows" className="type-h3">目錄列</h3>
        <div className="grid gap-3">
          <DirectoryRow
            variant="status"
            tone="active"
            stamp={<Stamp prefix="TAV." numeral={12} tone="aerugo" />}
            title="電車難題"
            status={<StatusChip tone="active">進行中</StatusChip>}
            meta={
              <>
                <Meta icon={School} variant="bibliographic" value="倫理學討論室" />
                <Meta icon={Clock} variant="bibliographic" value="今天 14:20" />
                <span className="md:hidden">
                  <Meta icon={MessageSquare} variant="bibliographic" value="8 回合" />
                </span>
              </>
            }
            trailing={
              <span className="inline-flex items-start gap-2.5">
                <span className="type-note hidden whitespace-nowrap text-ink-soft lg:inline">8 回合</span>
                <StageScale round={8} labels={stageLabels} ariaLabel="電車難題階段" />
              </span>
            }
            actions={
              <Button>
                繼續對話
                <ArrowRight aria-hidden="true" />
              </Button>
            }
          />
          <DirectoryRow
            variant="status"
            tone="draft"
            stamp={<Stamp prefix="ACTVM" numeral={2} tone="ochra" />}
            title="人數應是唯一判準嗎？"
            status={<StatusChip tone="draft">草稿</StatusChip>}
            meta={
              <>
                <Meta icon={CalendarClock} variant="bibliographic" value="尚未發布" />
                <span className="inline-flex items-center gap-1.5">
                  <Users size={14} strokeWidth={1.5} className="text-ink-soft" aria-hidden="true" />
                  <StackBar
                    size="sm"
                    showLegend={false}
                    ariaLabel="尚未有人參與"
                    className="w-24"
                    segments={[
                      { label: '已參與', value: 0, tone: 'caeruleum' },
                      { label: '未參與', value: 0, tone: 'track' },
                    ]}
                  />
                  <span className="type-note text-ink-soft">—</span>
                </span>
              </>
            }
            actions={
              <>
                <Button>
                  發布
                  <Send aria-hidden="true" />
                </Button>
                <Button variant="icon" aria-label="活動更多操作">
                  <Ellipsis aria-hidden="true" />
                </Button>
              </>
            }
          />
          <DirectoryRow
            variant="status"
            tone="ended"
            stamp={<Stamp prefix="TAV." numeral={10} tone="paper" />}
            title="民主一定比較公平嗎？"
            status={<StatusChip tone="inactive">已結束</StatusChip>}
            meta={
              <>
                <Meta icon={School} variant="bibliographic" value="倫理學討論室" />
                <Meta icon={Clock} variant="bibliographic" value="11/18" />
                <span className="md:hidden">
                  <Meta icon={MessageSquare} variant="bibliographic" value="15 回合" />
                </span>
              </>
            }
            trailing={
              <span className="inline-flex items-start gap-2.5">
                <span className="type-note hidden whitespace-nowrap text-ink-soft lg:inline">15 回合</span>
                <StageScale round={15} labels={stageLabels} ariaLabel="民主討論階段" />
              </span>
            }
            actions={
              <Button variant="quiet">
                <Eye aria-hidden="true" />
                回顧對話
              </Button>
            }
          />
          <DirectoryRow
            variant="topic"
            tone="topic"
            stamp={<Stamp prefix="QVAEST." numeral={1} tone="caeruleum" />}
            title="忒修斯之船"
            meta={
              <>
                <Meta icon={BookOpen} variant="bibliographic" value="題目書單 2 本" />
                <Meta icon={MessageSquare} variant="bibliographic" value="練習過 1 場" />
              </>
            }
            actions={
              <Button>
                開始練習
                <Play aria-hidden="true" />
              </Button>
            }
          />
          <SkeletonRegion loadingLabel="正在載入目錄列" className="grid gap-3">
            <DirectoryRowSkeleton variant="status" showStage />
            <DirectoryRowSkeleton variant="topic" />
          </SkeletonRegion>
        </div>
      </section>

      <section className="grid gap-3 border-t border-ink-faint pt-3" aria-labelledby="plate-cards">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <h3 id="plate-cards" className="type-h3">教室版畫卡</h3>
          <Button variant="quiet" onClick={() => setPlateCardLoaded((loaded) => !loaded)}>
            {plateCardLoaded ? '重置骨架' : '模擬載入完成'}
          </Button>
        </div>
        <div className="grid gap-4">
          <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3" data-testid="plate-card-grid">
            <PlateCard
              numeral={1}
              title="倫理學討論室"
              headerAction={
                <Button variant="icon" aria-label="倫理學討論室更多操作">
                  <Ellipsis aria-hidden="true" />
                </Button>
              }
              meta={
                <>
                  <Meta icon={Users} variant="bibliographic" value="32 位學生" />
                  <Meta icon={ListChecks} variant="bibliographic" value="2 個進行中活動" />
                  <Meta icon={CalendarClock} variant="bibliographic" value="下個截止 11/30" />
                </>
              }
              action={
                <Button>
                  進入教室
                  <ArrowRight aria-hidden="true" />
                </Button>
              }
            />
            <PlateCard
              numeral={2}
              title="哲學概論・週二班"
              meta={<Meta icon={ListChecks} variant="bibliographic" value="目前沒有活動" />}
              action={
                <Button>
                  進入教室
                  <ArrowRight aria-hidden="true" />
                </Button>
              }
            />
            <PlateCard
              numeral={3}
              title="邏輯導論（封存範例）"
              archived
              meta={
                <>
                  <Meta icon={ListChecks} variant="bibliographic" value="6 個活動" />
                  <Meta icon={Archive} variant="bibliographic" value="已封存・唯讀" />
                </>
              }
              action={
                <Button variant="quiet">
                  <Eye aria-hidden="true" />
                  查看教室
                </Button>
              }
            />
          </div>
          <div
            data-testid="plate-card-swap"
            aria-busy={!plateCardLoaded || undefined}
            className="grid gap-4 md:grid-cols-2 lg:grid-cols-3"
          >
            {plateCardLoaded ? (
              <PlateCard
                numeral={4}
                title="政治哲學導論"
                headerAction={
                  <Button variant="icon" aria-label="政治哲學導論更多操作">
                    <Ellipsis aria-hidden="true" />
                  </Button>
                }
                meta={
                  <>
                    <Meta icon={Users} variant="bibliographic" value="32 位學生" />
                    <Meta icon={ListChecks} variant="bibliographic" value="2 個進行中活動" />
                    <Meta icon={CalendarClock} variant="bibliographic" value="下個截止 11/30" />
                  </>
                }
                action={
                  <Button>
                    進入教室
                    <ArrowRight aria-hidden="true" />
                  </Button>
                }
              />
            ) : (
              <PlateCardSkeleton />
            )}
          </div>
          {!plateCardLoaded && (
            <span className="sr-only" role="status">正在載入教室卡片</span>
          )}
        </div>
      </section>

      <section className="grid gap-3 border-t border-ink-faint pt-3" aria-labelledby="books">
        <h3 id="books" className="type-h3">書目與共用書庫</h3>
        <div className="flex flex-wrap items-end gap-4">
          <BookCover size="sm" />
          <BookCover size="md" />
          <BookCover size="lg" placeholderLabel="封面" />
          <BookCoverSkeleton size="md" />
        </div>
        <div className="grid gap-3">
          <WorkRow
            title="正義：一場思辨之旅"
            status={<StatusChip tone="active">啟用</StatusChip>}
            defaultExpanded
            expandLabel="展開 2 個版本"
            collapseLabel="收合 2 個版本"
            meta={
              <>
                <Meta icon={BookOpen} variant="bibliographic" value="2 個版本" />
                <span className="hidden md:inline-flex">
                  <Meta icon={Languages} variant="bibliographic" value="中文・英文" />
                </span>
                <Meta icon={ListChecks} variant="bibliographic" value="2 份書單" />
              </>
            }
            actions={
              <>
                <Button variant="icon" className="hidden md:inline-flex" aria-label="找其他版本">
                  <Search aria-hidden="true" />
                </Button>
                <Button variant="icon" className="hidden md:inline-flex" aria-label="合併或移動版本">
                  <Network aria-hidden="true" />
                </Button>
                <Button variant="icon" aria-label="作品更多操作"><Ellipsis aria-hidden="true" /></Button>
              </>
            }
          >
            <EditionRow
              language="中文"
              title="正義：一場思辨之旅"
              meta={
                <>
                  <Meta icon={User} variant="bibliographic" value="Michael Sandel" />
                  <Meta icon={Clock} variant="bibliographic" value="2011" />
                  <span className="hidden md:inline-flex">
                    <Meta icon={FileText} variant="bibliographic" value="Google Books" />
                  </span>
                  <span className="hidden md:inline-flex">
                    <Meta icon={Pencil} variant="bibliographic" value="林教師新增" />
                  </span>
                </>
              }
              actions={
                <>
                  <Button variant="icon" aria-label="查看中文版本"><Eye aria-hidden="true" /></Button>
                  <Button variant="icon" aria-label="編輯中文版本"><Pencil aria-hidden="true" /></Button>
                </>
              }
            />
            <EditionRow
              language="英文"
              title="Justice: What's the Right Thing to Do?"
              meta={
                <>
                  <Meta icon={User} variant="bibliographic" value="Michael J. Sandel" />
                  <Meta icon={Clock} variant="bibliographic" value="2009" />
                  <span className="hidden md:inline-flex">
                    <Meta icon={FileText} variant="bibliographic" value="Open Library" />
                  </span>
                </>
              }
              actions={
                <Button variant="icon" aria-label="查看英文版本"><Eye aria-hidden="true" /></Button>
              }
            />
          </WorkRow>
          <WorkRow
            title="理想國"
            inactive
            status={<StatusChip tone="inactive">已停用</StatusChip>}
            expandLabel="展開 1 個版本"
            collapseLabel="收合 1 個版本"
            meta={
              <>
                <Meta icon={BookOpen} variant="bibliographic" value="1 個版本" />
                <Meta icon={ListChecks} variant="bibliographic" value="1 份書單" />
              </>
            }
            actions={
              <Button variant="icon" aria-label="理想國更多操作"><Ellipsis aria-hidden="true" /></Button>
            }
          >
            <EditionRow
              language="中文"
              title="理想國"
              meta={<Meta icon={User} variant="bibliographic" value="柏拉圖" />}
              actions={<Button variant="icon" aria-label="查看理想國版本"><Eye aria-hidden="true" /></Button>}
            />
          </WorkRow>
          <SkeletonRegion loadingLabel="正在載入共用書庫" className="grid gap-3">
            <WorkRowSkeleton editionsCount={1} />
          </SkeletonRegion>
        </div>
      </section>

      <section className="grid gap-3 border-t border-ink-faint pt-3" aria-labelledby="stack-bars">
        <h3 id="stack-bars" className="type-h3">比例長條</h3>
        <div className="grid gap-4 md:grid-cols-2">
          <div className="grid gap-3 border border-ink bg-surface p-4">
            <div className="flex items-center gap-3 md:gap-4">
              <Stamp prefix="QVAEST." numeral={1} tone="caeruleum" />
              <div className="flex min-w-0 flex-1 flex-col gap-1.5">
                <p className="type-h3 text-ink">為救五人該犧牲一人嗎？</p>
                <Meta icon={Users} variant="bibliographic" value="28 人參與" />
              </div>
            </div>
            <StackBar ariaLabel="為救五人該犧牲一人嗎 論點分布" segments={argumentSegments} />
          </div>
          <div className="grid content-start gap-4 border border-ink bg-surface p-4">
            <div className="flex items-center gap-2">
              <Users size={14} strokeWidth={1.5} className="shrink-0 text-ink-soft" aria-hidden="true" />
              <StackBar
                size="sm"
                showLegend={false}
                ariaLabel="22 / 28 人已參與"
                className="flex-1"
                segments={[
                  { label: '已參與', value: 22, tone: 'caeruleum' },
                  { label: '未參與', value: 6, tone: 'track' },
                ]}
              />
              <span className="type-note shrink-0 text-ink-soft">22／28</span>
            </div>
            <StackBar
              ariaLabel="尚無資料的論點分布"
              emptyLabel="尚無資料"
              segments={[
                { label: '立場甲', value: 0 },
                { label: '立場乙', value: 0 },
              ]}
            />
            <SkeletonRegion loadingLabel="正在載入比例長條">
              <StackBarSkeleton legendCount={3} />
            </SkeletonRegion>
          </div>
        </div>
      </section>
    </main>
  )
}
