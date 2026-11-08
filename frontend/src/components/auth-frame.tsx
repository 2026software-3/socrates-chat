import type { ReactNode } from 'react'
import { LangSwitcher } from '@/components/lang-switcher'
import { useT } from '@/i18n'

/** 登入前後的單卡頁面外框：頂欄（品牌、語言）＋置中的卡片；`footer` 為頁尾說明。 */
export function AuthFrame({ children, footer }: { children: ReactNode; footer?: ReactNode }) {
  const t = useT()
  return (
    <div className="flex min-h-svh flex-col">
      <header className="flex h-[52px] items-center justify-between border-b px-6">
        <span className="text-lg font-medium">{t('app.brand')}</span>
        <LangSwitcher />
      </header>
      <main className="flex flex-1 items-center justify-center px-4 py-10">{children}</main>
      {footer ? <footer className="text-muted-foreground border-t px-6 py-4 text-center text-xs">{footer}</footer> : null}
    </div>
  )
}

/** AuthFrame 內的卡片：400px 寬、32px 內距、10px 圓角。 */
export function AuthCard({ children, ...props }: { children: ReactNode } & React.ComponentProps<'section'>) {
  return (
    <section className="bg-background flex w-full max-w-[400px] flex-col gap-6 rounded-xl border p-8" {...props}>
      {children}
    </section>
  )
}

/** 卡片標題區：24px 標題＋14px 說明。 */
export function AuthHeading({ id, title, description }: { id?: string; title: string; description?: ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <h1 id={id} className="text-2xl font-medium">
        {title}
      </h1>
      {description ? <p className="text-muted-foreground text-sm">{description}</p> : null}
    </div>
  )
}
