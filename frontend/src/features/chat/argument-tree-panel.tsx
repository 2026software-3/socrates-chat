import { useMemo, useState } from 'react'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { errorText, useT } from '@/i18n'
import { api } from '@/lib/api'
import { usePollJob } from '@/lib/ai-jobs'
import type { PollSpec } from '@/lib/poll-store'
import { markAiUsageStale } from '@/lib/ai-usage-events'

type NodeKind = 'claim' | 'premise' | 'boundary' | 'rebuttal' | 'precision' | 'independent_judgment' | 'opposing_proposition'
type Relation = 'support' | 'attack' | 'qualify' | 'precision' | 'reflect' | 'context_only'
export type ArgumentNode = {
  id: string
  kind: NodeKind
  label: string
  source_round: number
  evidence: string
  parent: string | null
  relation: Relation | null
  origin: 'user' | 'assistant_question_scenario'
  children: ArgumentNode[]
}
type Tree = { topic_id: string; title: string; node_count: number; review_status: string; root: ArgumentNode }
type TreeView = {
  status: 'missing' | 'pending' | 'ready' | 'failed'
  tree: Tree | null
  prompt_version: string | null
  completed_at: string | null
}
type ViewState =
  | { kind: 'loading' | 'pending' | 'failed' | 'slow' }
  | { kind: 'ready'; tree: Tree }
  | { kind: 'error'; error: unknown }

const POLL_MS = 1000
const MAX_POLL_MS = 15 * 60 * 1000

/**
 * 讀取論證樹；尚無結果（missing）時接著 POST 啟動，POST 本身對 pending／ready 結果是冪等的。
 * `postFirst` 用於失敗後的明確重試：第一次直接 POST，之後改用 GET 輪詢。
 */
function treeSpec(conversationId: string, postFirst: boolean): PollSpec<TreeView> {
  const path = '/api/conversations/' + conversationId + '/argument-tree'
  let posted = !postFirst
  const post = async (signal: AbortSignal) => {
    try {
      return await api.post<TreeView>(path, undefined, signal)
    } finally {
      markAiUsageStale()
    }
  }
  return {
    fetch: async (signal) => {
      if (!posted) {
        posted = true
        return post(signal)
      }
      const result = await api.get<TreeView>(path, signal)
      return result.status === 'missing' ? post(signal) : result
    },
    next: (r) => (r.status === 'pending' ? 'poll-limited' : 'done'),
    intervalMs: POLL_MS,
    maxMs: MAX_POLL_MS,
  }
}

/**
 * Only explicit opening starts requests. 輪詢由全域登記處持有（S-20）：關閉視窗或離頁後，
 * 後端工作與輪詢都繼續，再打開時直接接上。
 */
export function ArgumentTreePanel({ conversationId, topicTitle }: { conversationId: string; topicTitle: string }) {
  const t = useT()
  const [open, setOpen] = useState(false)
  const [started, setStarted] = useState(false)
  const spec = useMemo(() => treeSpec(conversationId, false), [conversationId])
  const { state: poll, restart } = usePollJob(started ? `argtree:${conversationId}` : null, spec)

  const state: ViewState = poll.error !== undefined ? { kind: 'error', error: poll.error }
    : poll.data?.status === 'ready' && poll.data.tree ? { kind: 'ready', tree: poll.data.tree }
    : poll.data?.status === 'failed' ? { kind: 'failed' }
    : poll.timedOut ? { kind: 'slow' }
    : poll.data ? { kind: 'pending' }
    : { kind: 'loading' }

  const busy = open && (state.kind === 'loading' || state.kind === 'pending')
  return (
    <Dialog open={open} onOpenChange={(next) => {
      // 再次打開時，已停止的工作重新讀一次已保存的結果；仍在輪詢的直接接上
      if (next && started && !poll.polling) restart({})
      if (next) setStarted(true)
      setOpen(next)
    }}>
      <DialogTrigger asChild>
        <Button variant="outline" disabled={busy}>{t('chat.tree.button')}</Button>
      </DialogTrigger>
      <DialogContent className="max-h-[90dvh] overflow-y-auto sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle>{t('chat.tree.title')}</DialogTitle>
          <DialogDescription>{t('chat.tree.note')}</DialogDescription>
        </DialogHeader>
        {state.kind === 'loading' || state.kind === 'pending' ? (
          <p role="status" className="py-6 text-sm text-muted-foreground">{t('chat.tree.pending')}</p>
        ) : null}
        {state.kind === 'failed' || state.kind === 'error' ? (
          <Alert variant="destructive" role="alert">
            <AlertTitle>{t('chat.tree.failed')}</AlertTitle>
            {state.kind === 'error' ? <AlertDescription>{errorText(t, state.error)}</AlertDescription> : null}
            <Button variant="outline" className="mt-3" onClick={() => restart({ spec: treeSpec(conversationId, true) })}>
              {t('chat.tree.retry')}
            </Button>
          </Alert>
        ) : null}
        {state.kind === 'slow' ? (
          <Alert role="status">
            <AlertTitle>{t('chat.tree.slow')}</AlertTitle>
            <Button variant="outline" className="mt-3" onClick={() => restart({})}>
              {t('chat.tree.check')}
            </Button>
          </Alert>
        ) : null}
        {state.kind === 'ready' ? (
          <div className="min-w-0 space-y-4">
            <p className="text-sm font-medium">{topicTitle} · {t('chat.tree.count', { count: state.tree.node_count })}</p>
            <ul className="min-w-0" aria-label={t('chat.tree.title')}><TreeNode node={state.tree.root} root /></ul>
            <details className="rounded-lg border p-3">
              <summary className="cursor-pointer text-sm font-medium">{t('chat.tree.json')}</summary>
              <pre data-testid="argument-tree-json" className="mt-3 max-h-96 overflow-auto text-xs whitespace-pre-wrap break-all">
                {JSON.stringify(state.tree, null, 2)}
              </pre>
            </details>
          </div>
        ) : null}
      </DialogContent>
    </Dialog>
  )
}

function TreeNode({ node, root = false }: { node: ArgumentNode; root?: boolean }) {
  const t = useT()
  const kindKey = ('chat.tree.kind.' + node.kind) as import('@/i18n').MessageKey
  const relationKey = ('chat.tree.relation.' + node.relation) as import('@/i18n').MessageKey
  return (
    <li className="relative min-w-0">
      {!root ? (
        <svg aria-hidden="true" className="absolute -left-5 top-0 text-border" width="20" height="26" fill="none">
          <path d="M1 0V18H18" stroke="currentColor" strokeWidth="2" />
        </svg>
      ) : null}
      <details open className="mb-3 rounded-lg border bg-card p-3">
        <summary className="cursor-pointer break-words text-sm font-medium">
          <span>{node.label}</span>
          <span className="ml-2 inline-flex flex-wrap gap-1 align-middle">
            <Badge variant={root ? 'default' : 'secondary'}>{t(kindKey)}</Badge>
            {node.relation ? <Badge variant="outline">{t(relationKey)}</Badge> : null}
          </span>
        </summary>
        <p className="mt-2 text-xs text-muted-foreground">{t('chat.tree.round', { count: node.source_round })}</p>
        <blockquote className="mt-1 border-l-2 pl-3 text-sm whitespace-pre-wrap break-words">{node.evidence}</blockquote>
        {node.children.length > 0 ? (
          <ul className="mt-3 ml-2 border-l pl-5">{node.children.map((child) => <TreeNode key={child.id} node={child} />)}</ul>
        ) : null}
      </details>
    </li>
  )
}
