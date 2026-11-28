import { useEffect, useState } from 'react'
import { AdminStatusBadge, type AdminTone } from '@/features/admin/admin-ui'
import { useT, type MessageKey } from '@/i18n'

export type Health = 'ok' | 'unavailable' | 'unknown'

export function healthOf(value: string | undefined): Health {
  if (value === 'ok') return 'ok'
  if (value === undefined) return 'unknown'
  return 'unavailable'
}

const HEALTH_LABEL: Record<Health, MessageKey> = {
  ok: 'admin.overview.state.ok',
  unavailable: 'admin.overview.state.unavailable',
  unknown: 'admin.overview.state.unknown',
}

const HEALTH_TONE: Record<Health, AdminTone> = {
  ok: 'active',
  unavailable: 'error',
  unknown: 'inactive',
}

export function HealthBadge({ state }: { state: Health }) {
  const t = useT()
  return <AdminStatusBadge tone={HEALTH_TONE[state]}>{t(HEALTH_LABEL[state])}</AdminStatusBadge>
}

/**
 * 資料庫故障時管理者 session 驗證不了，狀態端點會失敗；這時改讀公開的 `/health` 與 `/ready`，
 * 至少顯示服務與資料庫是否正常，整頁不會失敗（S-09.3）。
 */
export function useBasicHealth(enabled: boolean): { service: Health; database: Health } | undefined {
  const [state, setState] = useState<{ service: Health; database: Health }>()
  useEffect(() => {
    if (!enabled) return
    const ctrl = new AbortController()
    const probe = async (path: string): Promise<Health> => {
      try {
        const res = await fetch(path, { signal: ctrl.signal, credentials: 'same-origin' })
        if (res.ok) return 'ok'
        return res.status === 503 ? 'unavailable' : 'unknown'
      } catch {
        return 'unknown'
      }
    }
    void Promise.all([probe('/health'), probe('/ready')]).then(([service, database]) => {
      if (!ctrl.signal.aborted) setState({ service, database })
    })
    return () => ctrl.abort()
  }, [enabled])
  return state
}

export function HealthItem({ label, state }: { label: string; state: Health }) {
  return (
    <div className="flex items-center gap-2">
      <span className="type-note text-ink-soft">{label}</span>
      <HealthBadge state={state} />
    </div>
  )
}
