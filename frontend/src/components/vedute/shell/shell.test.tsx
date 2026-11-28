import { readFileSync } from 'node:fs'
import path from 'node:path'
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { MemoryRouter } from 'react-router'
import { toast } from 'sonner'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { I18nProvider } from '@/i18n'
import { makeMe, mockMe, renderApp } from '@/test/render'
import { server } from '@/test/server'
import {
  AREAS_BY_ROLE,
  BackHeader,
  CarvedInscription,
  Colophon,
  ForumPlan,
  PageHeader,
  PageTitle,
  PlanSheet,
  Sidebar,
  TabBar,
  Topbar,
  resolveCurrentArea,
  resolveFocusConfig,
  resolveShellRole,
} from './index'

function mockViewport(initialWidth: number, reducedMotion = false) {
  let width = initialWidth
  const entries = new Map<string, { mql: MediaQueryList; listeners: Set<() => void> }>()
  const evaluate = (query: string) => {
    if (query.includes('prefers-reduced-motion')) return reducedMotion
    const min = query.match(/min-width:\s*(\d+)px/)
    const max = query.match(/max-width:\s*(\d+)px/)
    if (min && width < Number(min[1])) return false
    if (max && width > Number(max[1])) return false
    return true
  }
  const matchMedia = vi.fn((query: string): MediaQueryList => {
    const existing = entries.get(query)
    if (existing) return existing.mql
    const listeners = new Set<() => void>()
    const mql: MediaQueryList = {
      get matches() {
        return evaluate(query)
      },
      media: query,
      onchange: null,
      addEventListener: (_: string, cb: EventListenerOrEventListenerObject) => {
        if (typeof cb === 'function') listeners.add(cb as () => void)
      },
      removeEventListener: (_: string, cb: EventListenerOrEventListenerObject) => {
        if (typeof cb === 'function') listeners.delete(cb as () => void)
      },
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    }
    entries.set(query, { mql, listeners })
    return mql
  })
  vi.stubGlobal('matchMedia', matchMedia)
  return {
    resize(nextWidth: number) {
      width = nextWidth
      act(() => {
        for (const { listeners } of entries.values()) {
          for (const cb of listeners) cb()
        }
      })
    },
  }
}

afterEach(() => {
  vi.unstubAllGlobals()
})

describe('shell area definitions and route resolution', () => {
  it('defines exact Roman-numbered legend items per role matching DESIGN.md', () => {
    expect(AREAS_BY_ROLE.student.map((a) => [a.numeral, a.building, a.to])).toEqual([
      ['I', 'basilica', '/'],
      ['II', 'vesta', '/#practice'],
      ['III', 'rostra', '/conversations'],
      ['IV', 'saturn', '/tasks'],
      ['V', 'castor', '/parts'],
      ['VI', 'tabularium', '/account'],
    ])
    expect(AREAS_BY_ROLE.teacher.map((a) => [a.numeral, a.building, a.to])).toEqual([
      ['I', 'basilica', '/teacher/classrooms'],
      ['II', 'tabularium', '/teacher/topics'],
      ['III', 'saturn', '/teacher/books'],
      ['IV', 'curia', '/teacher/roster'],
      ['V', 'rostra', '/teacher/summaries'],
    ])
    expect(AREAS_BY_ROLE.admin.map((a) => [a.numeral, a.building, a.to])).toEqual([
      ['I', 'curia', '/admin'],
      ['II', 'basilica', '/admin/classrooms'],
      ['III', 'castor', '/admin/teachers'],
      ['IV', 'saturn', '/admin/books'],
      ['V', 'tabularium', '/admin/topics'],
      ['VI', 'vesta', '/admin/system-ai'],
      ['VII', 'rostra', '/admin/diagnostics'],
    ])
  })

  it('resolves role by current path for multi-role accounts', () => {
    const both = { student: false, teacher: true, admin: true }
    expect(resolveShellRole('/admin/teachers', both)).toBe('admin')
    expect(resolveShellRole('/teacher/classrooms', both)).toBe('teacher')
    expect(resolveShellRole('/account', both)).toBe('teacher')

    const all = { student: true, teacher: true, admin: true }
    expect(resolveShellRole('/', all)).toBe('student')
    expect(resolveShellRole('/teacher/topics', all)).toBe('teacher')
    expect(resolveShellRole('/admin/diagnostics', all)).toBe('admin')
  })

  it('resolves current area including grouped sub-pages and #practice anchor', () => {
    expect(resolveCurrentArea('student', '/', '')?.id).toBe('classrooms')
    expect(resolveCurrentArea('student', '/', '#practice')?.id).toBe('practice')
    expect(resolveCurrentArea('student', '/conversations', '')?.id).toBe('conversations')
    expect(resolveCurrentArea('student', '/tasks', '')?.id).toBe('tasks')
    expect(resolveCurrentArea('student', '/parts', '')?.id).toBe('companion')
    expect(resolveCurrentArea('student', '/account', '')?.id).toBe('profile')

    expect(resolveCurrentArea('teacher', '/teacher/keys', '')?.id).toBe('classrooms')
    expect(resolveCurrentArea('admin', '/admin/admins', '')?.id).toBe('teachers')
    expect(resolveCurrentArea('admin', '/admin/teachers/keys', '')?.id).toBe('teachers')
    expect(resolveCurrentArea('admin', '/admin/announcements', '')?.id).toBe('settings')
    expect(resolveCurrentArea('admin', '/admin/tasks', '')?.id).toBe('settings')
    expect(resolveCurrentArea('admin', '/admin/parts', '')?.id).toBe('settings')
    expect(resolveCurrentArea('admin', '/admin/services', '')?.id).toBe('logs')
    expect(resolveCurrentArea('admin', '/admin/metrics', '')?.id).toBe('logs')
    expect(resolveCurrentArea('admin', '/admin/complaints', '')?.id).toBe('logs')
  })

  it('identifies focus routes (conversation, transcript, student summary)', () => {
    expect(resolveFocusConfig('/conversations/c1', 'student')).toMatchObject({
      backTo: '/conversations',
      numeral: 'III',
      building: 'rostra',
    })
    expect(resolveFocusConfig('/teacher/classrooms/r1/conversations/c1', 'teacher')).toMatchObject({
      backTo: '/teacher/classrooms/r1',
      numeral: 'V',
      building: 'rostra',
    })
    expect(resolveFocusConfig('/admin/classrooms/r1/students/s1', 'admin')).toMatchObject({
      backTo: '/admin/classrooms/r1/students',
      numeral: 'VII',
      building: 'rostra',
    })
    expect(resolveFocusConfig('/conversations', 'student')).toBeNull()
    expect(resolveFocusConfig('/teacher/classrooms/r1', 'teacher')).toBeNull()
  })
})

describe('Vedute shell primitives', () => {
  it('carves Latin inscription with interpuncts and per-glyph stagger indices', () => {
    const { container } = render(<CarvedInscription text="Basilica Ivlia" delayMs={380} animateKey="basilica" />)
    const root = container.querySelector('[data-vedute-inscription]')
    expect(root).toHaveAttribute('aria-hidden', 'true')
    expect(root?.getAttribute('style')).toContain('--chisel-delay: 380ms')
    expect(root?.textContent).toBe('BASILICA·IVLIA')
    const glyphs = root?.querySelectorAll('[data-glyph]')
    expect(glyphs?.length).toBe('BASILICA·IVLIA'.length)
  })

  it('opens the current building on the Nolli ForumPlan and marks SVG aria-hidden', () => {
    const onSelect = vi.fn()
    const { container } = render(
      <ForumPlan
        areas={AREAS_BY_ROLE.student}
        currentAreaId="conversations"
        onSelectArea={onSelect}
      />,
    )
    const svg = container.querySelector('svg')
    expect(svg).toHaveAttribute('aria-hidden', 'true')
    const rostra = container.querySelector('[data-building="rostra"]')
    expect(rostra).toHaveClass('is-here')
    expect(container.querySelector('[data-building="curia"]')).toHaveClass('is-inert')
    expect(container.querySelector('[data-vedute-plan-pin]')?.textContent).toBe('III')

    fireEvent.click(container.querySelector('[data-building="tasks"]') ?? container.querySelector('[data-building="saturn"]')!)
    expect(onSelect).toHaveBeenCalledWith(expect.objectContaining({ id: 'tasks' }))
  })

  it('renders PageHeader with responsive AVIF/WebP plate, area wash, fade, numeral pin, and carved Latin', () => {
    const { container } = render(
      <I18nProvider initial="zh-TW">
        <MemoryRouter>
          <PageHeader
            title="我的教室"
            subtitle="本週有 2 個討論活動"
            area={AREAS_BY_ROLE.student[0]}
          />
        </MemoryRouter>
      </I18nProvider>,
    )
    expect(screen.getByRole('heading', { level: 1, name: '我的教室' })).toBeInTheDocument()
    const avif = container.querySelector('picture source[type="image/avif"]')
    const webp = container.querySelector('picture source[type="image/webp"]')
    expect(avif).not.toBeNull()
    expect(webp).not.toBeNull()
    expect(avif?.getAttribute('srcset')).toContain('768w')
    expect(avif?.getAttribute('srcset')).toContain('1440w')
    expect(container.querySelector('[data-vedute-wash="aerugo"]')).not.toBeNull()
    expect(container.querySelector('.vedute-page-header-fade')).not.toBeNull()
    expect(container.querySelector('[data-vedute-header-pin]')?.textContent).toBe('I')
  })

  it('renders PageTitle with optional eyebrow, Latin inscription, and note', () => {
    render(<PageTitle eyebrow="教室名稱" title="哲學概論" latin="BASILICA·IVLIA" note="唯讀" />)
    expect(screen.getByRole('heading', { level: 1, name: '哲學概論' })).toBeInTheDocument()
    expect(screen.getByText('教室名稱')).toBeInTheDocument()
    expect(screen.getByText('BASILICA·IVLIA')).toBeInTheDocument()
    expect(screen.getByText('唯讀')).toBeInTheDocument()
  })

  it('renders BackHeader with back link, title, and carved location', () => {
    render(
      <I18nProvider initial="zh-TW">
        <MemoryRouter>
          <BackHeader
            backTo="/conversations"
            backLabel="返回我的對話"
            title="民主一定比較公平嗎？"
            numeral="III"
            latin="Rostra"
            detail="講壇・第 5 輪"
          />
        </MemoryRouter>
      </I18nProvider>,
    )
    expect(screen.getByRole('link', { name: /返回我的對話/ })).toHaveAttribute('href', '/conversations')
    expect(screen.getByText('民主一定比較公平嗎？')).toBeInTheDocument()
    expect(screen.getByText('講壇・第 5 輪')).toBeInTheDocument()
  })

  it('renders Colophon with guide/privacy coming-soon toast and role-gated report link', async () => {
    const infoSpy = vi.spyOn(toast, 'info')
    const { rerender } = render(
      <I18nProvider initial="zh-TW">
        <MemoryRouter>
          <Colophon showReportLink />
        </MemoryRouter>
      </I18nProvider>,
    )
    expect(screen.getByRole('contentinfo')).toBeInTheDocument()
    expect(screen.getByText('SCHOLA SOCRATICA · MMXXVI')).toBeInTheDocument()
    expect(screen.getByRole('link', { name: '回報問題' })).toHaveAttribute('href', '/complaint')
    await userEvent.click(screen.getByRole('button', { name: '使用說明' }))
    await userEvent.click(screen.getByRole('button', { name: '紀錄與隱私' }))
    expect(infoSpy).toHaveBeenCalledWith('你發現了神秘頁面，敬請期待。')
    expect(infoSpy).toHaveBeenCalledTimes(2)
    infoSpy.mockRestore()

    rerender(
      <I18nProvider initial="zh-TW">
        <MemoryRouter>
          <Colophon showReportLink={false} />
        </MemoryRouter>
      </I18nProvider>,
    )
    expect(screen.queryByRole('link', { name: '回報問題' })).not.toBeInTheDocument()
  })
})

describe('drawer, plan sheet, focus management, and reduced motion', () => {
  it('renders TabBar with 3 frequent areas + Plaza, highlighting Plaza when on a non-frequent area', async () => {
    const user = userEvent.setup()
    const onOpenSheet = vi.fn()
    render(
      <I18nProvider initial="zh-TW">
        <MemoryRouter initialEntries={['/tasks']}>
          <TabBar
            role="student"
            areas={AREAS_BY_ROLE.student}
            currentArea={AREAS_BY_ROLE.student[3]}
            sheetOpen={false}
            onOpenSheet={onOpenSheet}
          />
        </MemoryRouter>
      </I18nProvider>,
    )
    const nav = screen.getByRole('navigation', { name: '主要區域' })
    expect(within(nav).getAllByRole('link')).toHaveLength(3)
    const plaza = within(nav).getByRole('button', { name: /IV・點數/ })
    expect(plaza).toHaveAttribute('aria-current', 'page')
    await user.click(plaza)
    expect(onOpenSheet).toHaveBeenCalledTimes(1)
  })

  it('traps focus in tablet drawer and returns focus to the menu trigger on Escape', async () => {
    mockViewport(768)
    mockMe(makeMe({ student: true }))
    server.use(
      http.get('/api/catalog', () => HttpResponse.json({ activities: [], topics: [] })),
      http.get('/api/points', () => HttpResponse.json({ balance: 128 })),
    )
    renderApp(null, { route: '/' })
    const menuBtn = await screen.findByRole('button', { name: '開啟導覽' })
    await userEvent.click(menuBtn)

    const dialog = await screen.findByRole('dialog', { name: '主導覽' })
    expect(dialog).toHaveAttribute('aria-modal', 'true')
    await waitFor(() => expect(dialog.contains(document.activeElement)).toBe(true))

    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(screen.queryByRole('dialog', { name: '主導覽' })).not.toBeInTheDocument())
    expect(menuBtn).toHaveFocus()
  })

  it('opens PlanSheet on mobile, traps focus, and returns focus to trigger on Escape', async () => {
    mockViewport(375)
    mockMe(makeMe({ student: true }))
    server.use(
      http.get('/api/catalog', () => HttpResponse.json({ activities: [], topics: [] })),
      http.get('/api/points', () => HttpResponse.json({ balance: 128 })),
    )
    renderApp(null, { route: '/' })
    const tabBar = await screen.findByRole('navigation', { name: '主要區域' })
    const plazaBtn = within(tabBar).getByRole('button', { name: '廣場' })
    await userEvent.click(plazaBtn)

    const sheet = await screen.findByRole('dialog', { name: /Pianta del Foro/i })
    expect(sheet).toHaveAttribute('aria-modal', 'true')
    await waitFor(() => expect(sheet.contains(document.activeElement)).toBe(true))

    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(screen.queryByRole('dialog', { name: /Pianta del Foro/i })).not.toBeInTheDocument())
    expect(plazaBtn).toHaveFocus()
  })

  it('disables animations in shell.css under prefers-reduced-motion: reduce and enables viewport-fit=cover', () => {
    const css = readFileSync(path.resolve(import.meta.dirname, 'shell.css'), 'utf8')
    expect(css).toMatch(/@media\s*\(prefers-reduced-motion:\s*reduce\)/)
    expect(css).toMatch(/animation:\s*none/)
    expect(css).toMatch(/transition:\s*none/)
    const html = readFileSync(path.resolve(import.meta.dirname, '../../../../index.html'), 'utf8')
    expect(html).toContain('viewport-fit=cover')
  })

  it('closes tablet drawer and mobile sheet when crossing breakpoints and does not resurrect them', async () => {
    const vp = mockViewport(768)
    mockMe(makeMe({ student: true }))
    server.use(
      http.get('/api/catalog', () => HttpResponse.json({ activities: [], topics: [] })),
      http.get('/api/points', () => HttpResponse.json({ balance: 128 })),
    )
    renderApp(null, { route: '/' })

    await userEvent.click(await screen.findByRole('button', { name: '開啟導覽' }))
    expect(await screen.findByRole('dialog', { name: '主導覽' })).toBeInTheDocument()

    // Crossing 768 -> 1024 closes the drawer; resizing back to 768 keeps it closed
    vp.resize(1024)
    await waitFor(() => expect(screen.queryByRole('dialog', { name: '主導覽' })).not.toBeInTheDocument())
    vp.resize(768)
    expect(screen.queryByRole('dialog', { name: '主導覽' })).not.toBeInTheDocument()

    // Crossing to 375 -> open PlanSheet -> resize to 768 closes sheet and releases trap
    vp.resize(375)
    const tabBar = screen.getByRole('navigation', { name: '主要區域' })
    await userEvent.click(within(tabBar).getByRole('button', { name: '廣場' }))
    expect(await screen.findByRole('dialog', { name: /Pianta del Foro/i })).toBeInTheDocument()

    vp.resize(768)
    await waitFor(() => expect(screen.queryByRole('dialog', { name: /Pianta del Foro/i })).not.toBeInTheDocument())
    vp.resize(375)
    expect(screen.queryByRole('dialog', { name: /Pianta del Foro/i })).not.toBeInTheDocument()
  })

  it('scrolls to #practice and scrolls back to top when returning to classrooms (/)', async () => {
    mockViewport(1440)
    mockMe(makeMe({ student: true }))
    server.use(
      http.get('/api/catalog', () =>
        HttpResponse.json({
          activities: [],
          topics: [{ id: 't1', title: '自由意志', description: 'desc', is_active: true, title_pending: false, pending_questions: [] }],
        }),
      ),
      http.get('/api/points', () => HttpResponse.json({ balance: 128 })),
    )
    const scrollSpy = vi.spyOn(Element.prototype, 'scrollIntoView')
    renderApp(null, { route: '/' })
    const nav = await screen.findByRole('navigation', { name: '主導覽' })
    await screen.findByText('自由意志')
    scrollSpy.mockClear()

    await userEvent.click(within(nav).getByRole('link', { name: /個人練習/ }))
    await waitFor(() =>
      expect(scrollSpy.mock.instances.some((el) => (el as HTMLElement).id === 'practice')).toBe(true),
    )

    scrollSpy.mockClear()
    await userEvent.click(within(nav).getByRole('link', { name: /我的教室/ }))
    await waitFor(() =>
      expect(scrollSpy.mock.instances.includes(document.documentElement)).toBe(true),
    )
    scrollSpy.mockRestore()
  })

  it('shows all landmarks in WorkShell and omits sidebar/footer in FocusShell', async () => {
    mockViewport(1440)
    mockMe(makeMe({ student: true }))
    server.use(
      http.get('/api/catalog', () => HttpResponse.json({ activities: [], topics: [] })),
      http.get('/api/points', () => HttpResponse.json({ balance: 128 })),
    )
    const view = renderApp(null, { route: '/' })
    expect(await screen.findByRole('banner')).toBeInTheDocument()
    expect(screen.getByRole('navigation', { name: '主導覽' })).toBeInTheDocument()
    expect(screen.getByRole('main')).toBeInTheDocument()
    expect(screen.getByRole('contentinfo')).toBeInTheDocument()
    view.unmount()

    server.use(
      http.get('/api/conversations/c1', () =>
        HttpResponse.json({
          id: 'c1',
          activity_id: null,
          activity_title: null,
          topic_id: 't1',
          title: '正義是什麼？',
          title_pending: false,
          description: '',
          language: 'zh-TW',
          status: 'active',
          stage: 1,
          turn_count: 1,
          converge_ready: false,
          created_at: '2026-11-01T00:00:00Z',
          updated_at: '2026-11-01T00:00:00Z',
          ended_at: null,
          is_archived: false,
          classroom_name: null,
          classroom_archived: false,
          messages: [
            { id: 'm1', role: 'student', content: '我認為正義是公平。', source: 'text', question_type: null, created_at: '2026-11-01T00:00:00Z' },
          ],
          generating: false,
          voice_realtime: false,
          guiding_questions: [],
        }),
      ),
    )
    renderApp(null, { route: '/conversations/c1' })
    const header = await screen.findByRole('banner')
    expect(within(header).getByRole('link', { name: /返回我的對話/ })).toBeInTheDocument()
    expect(await within(header).findByText('正義是什麼？')).toBeInTheDocument()
    expect(await within(header).findByText('講壇・第 1 輪')).toBeInTheDocument()
    expect(screen.queryByRole('navigation', { name: '主導覽' })).not.toBeInTheDocument()
    expect(screen.queryByRole('contentinfo')).not.toBeInTheDocument()
  })

  it('hides student points in Topbar when /api/points fails instead of showing 0', async () => {
    mockViewport(1440)
    mockMe(makeMe({ student: true }))
    server.use(
      http.get('/api/catalog', () => HttpResponse.json({ activities: [], topics: [] })),
      http.get('/api/points', () => HttpResponse.json({ error: { code: 'internal', request_id: 'r' } }, { status: 500 })),
    )
    renderApp(null, { route: '/' })
    await screen.findByRole('navigation', { name: '主導覽' })
    const banner = screen.getByRole('banner')
    expect(within(banner).queryByText('點數')).not.toBeInTheDocument()
  })

  it('renders Sidebar and Topbar standalone without errors', () => {
    const me = makeMe({ student: true })
    render(
      <I18nProvider initial="zh-TW">
        <MemoryRouter>
          <Topbar
            me={me}
            role="student"
            area={AREAS_BY_ROLE.student[0]}
            points={42}
            drawerOpen={false}
            sheetOpen={false}
            onOpenDrawer={() => {}}
            onOpenSheet={() => {}}
          />
          <Sidebar
            role="student"
            areas={AREAS_BY_ROLE.student}
            currentArea={AREAS_BY_ROLE.student[0]}
          />
          <PlanSheet
            open={false}
            role="student"
            areas={AREAS_BY_ROLE.student}
            currentArea={AREAS_BY_ROLE.student[0]}
            onClose={() => {}}
          />
        </MemoryRouter>
      </I18nProvider>,
    )
    expect(screen.getByText('42')).toBeInTheDocument()
  })
})
