import { fireEvent, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import {
  ArrowRight,
  BookOpen,
  CalendarClock,
  Clock,
  Ellipsis,
  Eye,
  ListChecks,
  Pencil,
  School,
  User,
} from 'lucide-react'
import { describe, expect, it, vi } from 'vitest'
import { BookCover, BookCoverSkeleton } from './book-cover'
import { Button } from './button'
import { DirectoryRow, DirectoryRowSkeleton } from './directory-row'
import { EditionRow, EditionRowSkeleton } from './edition-row'
import { Meta } from './meta'
import { PlateCard, PlateCardSkeleton } from './plate-card'
import { SkeletonRegion } from './skeleton'
import { StackBar, StackBarSkeleton } from './stack-bar'
import { StageScale, StageScaleSkeleton } from './stage-scale'
import { Stamp, StampSkeleton } from './stamp'
import { StatusChip } from './status-chip'
import { WorkRow, WorkRowSkeleton } from './work-row'

const STAGES = ['釐清', '論證', '挑戰'] as const

describe('Stamp', () => {
  it('renders prefix and numeral (string or integer) across tones and marks decorative by default', () => {
    const { container } = render(
      <>
        <Stamp prefix="TAV." numeral={12} tone="aerugo" />
        <Stamp prefix="QVAEST." numeral="IV" tone="caeruleum" size="sm" />
        <Stamp prefix="ACTVM" numeral={2} tone="ochra" ariaLabel="活動二" />
      </>,
    )

    expect(screen.getByText('TAV.')).toBeVisible()
    expect(screen.getByText('XII')).toBeVisible()
    expect(screen.getByText('QVAEST.')).toBeVisible()
    expect(screen.getByText('IV')).toBeVisible()
    expect(screen.getByText('II')).toBeVisible()

    const stamps = container.querySelectorAll('[data-vedute-stamp]')
    expect(stamps).toHaveLength(3)
    expect(stamps[0]).toHaveAttribute('aria-hidden', 'true')
    expect(stamps[0]).toHaveAttribute('data-tone', 'aerugo')
    expect(stamps[1]).toHaveAttribute('data-compact-prefix', 'true')
    expect(stamps[1]).toHaveAttribute('data-size', 'sm')
    expect(stamps[2]).toHaveAttribute('aria-label', '活動二')
    expect(stamps[2]).not.toHaveAttribute('aria-hidden')
  })

  it('renders StampSkeleton hidden from assistive technology', () => {
    const { container } = render(<StampSkeleton size="md" />)
    expect(container.querySelector('[aria-hidden="true"]')).not.toBeNull()
  })
})

describe('DirectoryRow', () => {
  it('renders status-line and topic-box variants with composed Vedute primitives', async () => {
    const user = userEvent.setup()
    const onContinue = vi.fn()
    const onPractice = vi.fn()

    const { container } = render(
      <>
        <DirectoryRow
          variant="status"
          tone="active"
          stamp={<Stamp prefix="TAV." numeral={12} tone="aerugo" />}
          title="電車難題"
          status={<StatusChip tone="active">進行中</StatusChip>}
          meta={
            <>
              <Meta icon={School} label="教室" value="軟體工程" />
              <Meta icon={Clock} label="時間" value="今天 14:20" />
            </>
          }
          trailing={<StageScale round={8} roundLabel="8 回合" labels={STAGES} ariaLabel="追問階段" />}
          actions={
            <Button onClick={onContinue}>
              繼續對話
              <ArrowRight aria-hidden="true" />
            </Button>
          }
        />
        <DirectoryRow
          variant="topic"
          tone="topic"
          stamp={<Stamp prefix="QVAEST." numeral={1} tone="caeruleum" />}
          title="忒修斯之船"
          meta={<Meta icon={BookOpen} variant="bibliographic" value="題目書單 2 本" />}
          actions={<Button onClick={onPractice}>開始練習</Button>}
        />
      </>,
    )

    const rows = container.querySelectorAll('[data-vedute-directory-row]')
    expect(rows).toHaveLength(2)
    expect(rows[0]).toHaveAttribute('data-variant', 'status')
    expect(rows[0]).toHaveAttribute('data-tone', 'active')
    expect(rows[1]).toHaveAttribute('data-variant', 'topic')
    expect(rows[1]).toHaveAttribute('data-tone', 'topic')

    expect(screen.getByText('電車難題')).toBeVisible()
    expect(screen.getByText('進行中')).toBeVisible()
    expect(screen.getByText('忒修斯之船')).toBeVisible()

    await user.tab()
    expect(screen.getByRole('button', { name: '繼續對話' })).toHaveFocus()
    await user.keyboard('{Enter}')
    expect(onContinue).toHaveBeenCalledOnce()

    await user.tab()
    expect(screen.getByRole('button', { name: '開始練習' })).toHaveFocus()
    await user.keyboard(' ')
    expect(onPractice).toHaveBeenCalledOnce()
  })

  it('renders DirectoryRowSkeleton inside a SkeletonRegion without tab stops', async () => {
    const user = userEvent.setup()
    const { container } = render(
      <>
        <SkeletonRegion loadingLabel="正在載入對話列表">
          <DirectoryRowSkeleton variant="status" showStage />
          <DirectoryRowSkeleton variant="topic" />
        </SkeletonRegion>
        <button>Next</button>
      </>,
    )

    expect(screen.getByRole('status')).toHaveTextContent('正在載入對話列表')
    expect(container.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThanOrEqual(6)
    await user.tab()
    expect(screen.getByRole('button', { name: 'Next' })).toHaveFocus()
  })
})

describe('PlateCard', () => {
  it('renders active and archived classroom cards with optional header action and full-width button', async () => {
    const user = userEvent.setup()
    const onEnter = vi.fn()
    const onMore = vi.fn()

    const { container } = render(
      <>
        <PlateCard
          numeral={1}
          title="軟體工程"
          headerAction={
            <Button variant="icon" aria-label="更多操作" onClick={onMore}>
              <Ellipsis aria-hidden="true" />
            </Button>
          }
          meta={
            <>
              <Meta icon={ListChecks} variant="bibliographic" value="2 個進行中活動" />
              <Meta icon={CalendarClock} variant="bibliographic" value="下個截止 11/30" />
            </>
          }
          action={
            <Button onClick={onEnter}>
              進入教室
              <ArrowRight aria-hidden="true" />
            </Button>
          }
        />
        <PlateCard
          numeral="II"
          title="2025 邏輯導論"
          archived
          action={
            <Button variant="quiet">
              <Eye aria-hidden="true" />
              查看教室
            </Button>
          }
        />
      </>,
    )

    expect(screen.getByText('CLASSIS I')).toBeVisible()
    expect(screen.getByText('CLASSIS II')).toBeVisible()
    expect(screen.getByText('軟體工程')).toBeVisible()
    expect(screen.getByText('2025 邏輯導論')).toBeVisible()

    const cards = container.querySelectorAll('[data-vedute-plate-card]')
    expect(cards[0]).not.toHaveAttribute('data-archived')
    expect(cards[1]).toHaveAttribute('data-archived', 'true')

    await user.click(screen.getByRole('button', { name: '更多操作' }))
    expect(onMore).toHaveBeenCalledOnce()
    await user.click(screen.getByRole('button', { name: '進入教室' }))
    expect(onEnter).toHaveBeenCalledOnce()
  })

  it('renders PlateCardSkeleton with aria-hidden placeholders', () => {
    const { container } = render(<PlateCardSkeleton />)
    expect(container.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThanOrEqual(4)
  })
})

describe('StageScale', () => {
  it('derives stage from round (including round 0) and highlights current stage in full mode', () => {
    const { rerender } = render(<StageScale round={0} labels={STAGES} ariaLabel="追問階段" />)
    const list = screen.getByRole('list', { name: '追問階段' })
    const items = within(list).getAllByRole('listitem')
    expect(items).toHaveLength(3)
    expect(items[0]).toHaveAttribute('aria-current', 'step')
    expect(items[1]).not.toHaveAttribute('aria-current')

    rerender(<StageScale round={8} roundLabel="8 回合" labels={STAGES} ariaLabel="追問階段" />)
    expect(screen.getByText('8 回合')).toBeVisible()
    const updated = within(screen.getByRole('list', { name: '追問階段' })).getAllByRole('listitem')
    expect(updated[1]).toHaveAttribute('aria-current', 'step')
    expect(updated[0]).toHaveAttribute('data-state', 'done')
    expect(updated[1]).toHaveAttribute('data-state', 'current')
    expect(updated[2]).toHaveAttribute('data-state', 'upcoming')
  })

  it('renders compact single-line variant with only the current stage label', () => {
    render(<StageScale variant="compact" round={3} roundLabel="回合 3" labels={STAGES} ariaLabel="追問階段" />)
    expect(screen.getByText('回合 3')).toBeVisible()
    expect(screen.getByText('釐清')).toBeVisible()
    expect(screen.queryByText('論證')).not.toBeInTheDocument()
    expect(screen.queryByText('挑戰')).not.toBeInTheDocument()
  })

  it('renders StageScaleSkeleton for full and compact variants', () => {
    const { container } = render(
      <>
        <StageScaleSkeleton variant="full" showRound />
        <StageScaleSkeleton variant="compact" showRound />
      </>,
    )
    expect(container.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThanOrEqual(5)
  })
})

describe('BookCover', () => {
  it('renders placeholder sizes and falls back to placeholder when cover image fails', () => {
    const { container, rerender } = render(
      <>
        <BookCover size="sm" />
        <BookCover size="md" />
        <BookCover size="lg" placeholderLabel="封面" />
      </>,
    )

    expect(screen.getByText('封面')).toBeVisible()
    expect(container.querySelectorAll('[data-vedute-book-cover]')).toHaveLength(3)

    rerender(<BookCover size="md" src="https://example.invalid/cover.jpg" alt="正義" />)
    const img = screen.getByRole('img', { name: '正義' })
    expect(img).toHaveAttribute('src', 'https://example.invalid/cover.jpg')
    fireEvent.error(img)
    expect(screen.queryByRole('img', { name: '正義' })).not.toBeInTheDocument()
    expect(container.querySelector('[data-vedute-book-cover]')).toHaveAttribute('data-placeholder', 'true')
  })

  it('renders BookCoverSkeleton for all sizes', () => {
    const { container } = render(
      <>
        <BookCoverSkeleton size="sm" />
        <BookCoverSkeleton size="md" />
        <BookCoverSkeleton size="lg" />
      </>,
    )
    expect(container.querySelectorAll('[data-slot="skeleton"]')).toHaveLength(3)
  })
})

describe('EditionRow and WorkRow', () => {
  it('renders EditionRow with paper language tag and icon-only actions', async () => {
    const user = userEvent.setup()
    const onView = vi.fn()
    const onEdit = vi.fn()

    render(
      <EditionRow
        language="中文"
        title="正義：一場思辨之旅"
        meta={
          <>
            <Meta icon={User} variant="bibliographic" value="Michael Sandel" />
            <Meta icon={Clock} variant="bibliographic" value="2011" />
          </>
        }
        actions={
          <>
            <Button variant="icon" aria-label="查看版本" onClick={onView}>
              <Eye aria-hidden="true" />
            </Button>
            <Button variant="icon" aria-label="編輯版本" onClick={onEdit}>
              <Pencil aria-hidden="true" />
            </Button>
          </>
        }
      />,
    )

    expect(screen.getByText('中文')).toBeVisible()
    expect(screen.getByText('正義：一場思辨之旅')).toBeVisible()
    expect(screen.getByText('Michael Sandel')).toBeVisible()

    await user.click(screen.getByRole('button', { name: '查看版本' }))
    expect(onView).toHaveBeenCalledOnce()
    await user.click(screen.getByRole('button', { name: '編輯版本' }))
    expect(onEdit).toHaveBeenCalledOnce()
  })

  it('toggles editions in WorkRow by pointer and keyboard, and handles works without editions', async () => {
    const user = userEvent.setup()
    render(
      <>
        <WorkRow
          title="正義：一場思辨之旅"
          status={<StatusChip tone="active">啟用</StatusChip>}
          meta={<Meta icon={BookOpen} variant="bibliographic" value="2 個版本" />}
          expandLabel="展開 2 個版本"
          collapseLabel="收合 2 個版本"
        >
          <EditionRow language="中文" title="正義：一場思辨之旅" />
          <EditionRow language="英文" title="Justice: What's the Right Thing to Do?" />
        </WorkRow>
        <WorkRow title="理想國" inactive status={<StatusChip tone="inactive">已停用</StatusChip>} />
      </>,
    )

    const toggle = screen.getByRole('button', { name: '展開 2 個版本' })
    expect(toggle).toHaveAttribute('aria-expanded', 'false')
    expect(screen.queryByText("Justice: What's the Right Thing to Do?")).not.toBeInTheDocument()

    toggle.focus()
    await user.keyboard('{Enter}')
    expect(screen.getByRole('button', { name: '收合 2 個版本' })).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByText("Justice: What's the Right Thing to Do?")).toBeVisible()

    await user.click(screen.getByRole('button', { name: '收合 2 個版本' }))
    expect(screen.getByRole('button', { name: '展開 2 個版本' })).toHaveAttribute('aria-expanded', 'false')
  })

  it('renders EditionRowSkeleton and WorkRowSkeleton cleanly', () => {
    const { container } = render(
      <>
        <WorkRowSkeleton editionsCount={2} />
        <EditionRowSkeleton />
      </>,
    )
    expect(container.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThanOrEqual(8)
  })
})

describe('StackBar', () => {
  it('applies No Right-or-Wrong default palette order and renders accessible legend', () => {
    const { container } = render(
      <StackBar
        ariaLabel="為救五人該犧牲一人嗎 論點分布"
        segments={[
          { label: '拉桿救五人', value: 14 },
          { label: '不介入', value: 8 },
          { label: '視情況', value: 4 },
          { label: '未表態', value: 2 },
        ]}
      />,
    )

    const bars = container.querySelectorAll('[data-vedute-segment]')
    expect(bars).toHaveLength(4)
    expect(bars[0]).toHaveAttribute('data-tone', 'caeruleum')
    expect(bars[1]).toHaveAttribute('data-tone', 'ochra')
    expect(bars[2]).toHaveAttribute('data-tone', 'aerugo')
    expect(bars[3]).toHaveAttribute('data-tone', 'neutral')

    expect(screen.getByText('拉桿救五人')).toBeVisible()
    expect(screen.getByText('14')).toBeVisible()
    expect(screen.getByText('未表態')).toBeVisible()
  })

  it('handles empty and zero-total datasets without NaN widths and supports compact inline mode', () => {
    const { container, rerender } = render(
      <StackBar
        ariaLabel="空分布"
        emptyLabel="尚無資料"
        segments={[
          { label: '立場 A', value: 0 },
          { label: '立場 B', value: 0 },
        ]}
      />,
    )

    expect(screen.getByText('尚無資料')).toBeVisible()
    expect(container.innerHTML).not.toContain('NaN')

    rerender(
      <StackBar
        size="sm"
        showLegend={false}
        ariaLabel="22 / 28 人已參與"
        segments={[
          { label: '已參與', value: 22, tone: 'caeruleum' },
          { label: '未參與', value: 6, tone: 'track' },
        ]}
      />,
    )
    expect(screen.getByRole('img', { name: '22 / 28 人已參與' })).toBeVisible()
    expect(screen.queryByText('已參與')).not.toBeInTheDocument()
  })

  it('renders StackBarSkeleton for full and inline variants', () => {
    const { container } = render(
      <>
        <StackBarSkeleton legendCount={3} />
        <StackBarSkeleton size="sm" showLegend={false} />
      </>,
    )
    expect(container.querySelectorAll('[data-slot="skeleton"]').length).toBeGreaterThanOrEqual(5)
  })
})
