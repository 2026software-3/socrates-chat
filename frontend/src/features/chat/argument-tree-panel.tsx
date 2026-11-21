import { useEffect, useState } from 'react'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from '@/components/ui/dialog'
import { errorText, useT } from '@/i18n'
import { api, isAbortError } from '@/lib/api'
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

/** Only explicit opening starts requests. POST is idempotent for pending/ready results. */
export function ArgumentTreePanel({ conversationId }: { conversationId: string }) {
  const t = useT()
  const [open, setOpen] = useState(false)
  const [state, setState] = useState<ViewState>({ kind: 'loading' })
  const [action, setAction] = useState({ revision: 0, retry: false })

  useEffect(() => {
    if (!open) return
    const ctrl = new AbortController()
    let timer: ReturnType<typeof setTimeout> | undefined
    const path = '/api/conversations/' + conversationId + '/argument-tree'
    const startedAt = Date.now()
    async function start() {
      try {
        const result = await api.post<TreeView>(path, undefined, ctrl.signal)
        markAiUsageStale()
        if (!ctrl.signal.aborted) accept(result)
      } catch (error) {
        markAiUsageStale()
        if (!ctrl.signal.aborted && !isAbortError(error)) setState({ kind: 'error', error })
      }
    }
    function accept(result: TreeView) {
      if (result.status === 'ready' && result.tree) setState({ kind: 'ready', tree: result.tree })
      else if (result.status === 'failed') setState({ kind: 'failed' })
      else if (result.status === 'missing') void start()
      else if (Date.now() - startedAt >= MAX_POLL_MS) setState({ kind: 'slow' })
      else {
        setState({ kind: 'pending' })
        timer = setTimeout(() => void read(), POLL_MS)
      }
    }
    async function read() {
      try {
        const result = await api.get<TreeView>(path, ctrl.signal)
        if (!ctrl.signal.aborted) accept(result)
      } catch (error) {
        if (!ctrl.signal.aborted && !isAbortError(error)) setState({ kind: 'error', error })
      }
    }
    if (action.retry) void start()
    else void read()
    return () => { ctrl.abort(); clearTimeout(timer) }
  }, [open, conversationId, action])

  const busy = open && (state.kind === 'loading' || state.kind === 'pending')
  return (
    <Dialog open={open} onOpenChange={(next) => {
      if (next) {
        setState({ kind: 'loading' })
        setAction((a) => ({ revision: a.revision + 1, retry: false }))
      }
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
            <Button variant="outline" className="mt-3" onClick={() => { setState({ kind: 'loading' }); setAction((a) => ({ revision: a.revision + 1, retry: true })) }}>
              {t('chat.tree.retry')}
            </Button>
          </Alert>
        ) : null}
        {state.kind === 'slow' ? (
          <Alert role="status">
            <AlertTitle>{t('chat.tree.slow')}</AlertTitle>
            <Button variant="outline" className="mt-3" onClick={() => { setState({ kind: 'loading' }); setAction((a) => ({ revision: a.revision + 1, retry: false })) }}>
              {t('chat.tree.check')}
            </Button>
          </Alert>
        ) : null}
        {state.kind === 'ready' ? (
          <div className="min-w-0 space-y-4">
            <p className="text-sm font-medium">{state.tree.title} · {t('chat.tree.count', { count: state.tree.node_count })}</p>
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
