import { useEffect, useState } from 'react'
import { useI18n } from '@/i18n'
import { api } from '@/lib/api'

export type PublicAnnouncement = { id: string; kind: 'general' | 'downtime'; text: string }
export type PublicAnnouncements = { maintenance: boolean; announcements: PublicAnnouncement[] }

/** 多久重新讀一次公告：管理者新發佈、停用或結束的公告，使用者不必重新整理就會更新。 */
const REFRESH_MS = 60_000

/**
 * 目前生效的公告（S-15）：不需要登入，語言跟著介面語言。
 * 讀取失敗時當作沒有公告（橫幅不該因為自己壞掉而吵到使用者），下一輪再試。
 */
export function useAnnouncements(): PublicAnnouncements | undefined {
  const { lang } = useI18n()
  const [data, setData] = useState<PublicAnnouncements>()

  useEffect(() => {
    let cancelled = false
    const ctrl = new AbortController()
    const load = () => {
      api
        .get<PublicAnnouncements>(`/api/announcements?lang=${encodeURIComponent(lang)}`, ctrl.signal)
        .then((d) => {
          if (!cancelled) setData(d)
        })
        .catch(() => {
          if (!cancelled) setData(undefined)
        })
    }
    load()
    const timer = setInterval(load, REFRESH_MS)
    return () => {
      cancelled = true
      ctrl.abort()
      clearInterval(timer)
    }
  }, [lang])

  return data
}
