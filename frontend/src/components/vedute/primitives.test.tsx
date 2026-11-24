import { act, fireEvent, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { Alert } from './alert'
import { Button } from './button'
import { Field } from './field'
import { Meta } from './meta'
import { Pagination } from './pagination'
import { Select } from './select'
import { Skeleton, SkeletonRegion } from './skeleton'
import { StatusChip } from './status-chip'
import { Switch } from './switch'
import { Tabs, TabsContent, TabsList, TabsTrigger } from './tabs'
import { Textarea } from './textarea'
import { Toast } from './toast'
import { User } from 'lucide-react'

it('activates all enabled button variants by keyboard and skips disabled ones', async () => {
  const user = userEvent.setup()
  const activate = vi.fn()
  render(
    <form onSubmit={(event) => event.preventDefault()}>
      <Button onClick={activate}>Save</Button>
      <Button disabled onClick={activate}>Unavailable</Button>
      <Button variant="quiet" onClick={activate}>Cancel</Button>
      <Button variant="ghost" onClick={activate}>Edit</Button>
      <Button variant="link" onClick={activate}>Details</Button>
      <Button variant="icon" aria-label="Account" onClick={activate}><User /></Button>
    </form>,
  )

  expect(screen.getByRole('button', { name: 'Save' })).toHaveAttribute('type', 'button')
  expect(screen.getByRole('button', { name: 'Unavailable' })).toBeDisabled()
  for (const name of ['Save', 'Cancel', 'Edit', 'Details', 'Account']) {
    await user.tab()
    expect(screen.getByRole('button', { name })).toHaveFocus()
    await user.keyboard('{Enter}')
  }
  expect(activate).toHaveBeenCalledTimes(5)
})

it('keeps link semantics when a button style is used for navigation', async () => {
  const user = userEvent.setup()
  const activate = vi.fn((event) => event.preventDefault())
  render(<Button variant="link" asChild><a href="/help" onClick={activate}>Help</a></Button>)

  const link = screen.getByRole('link', { name: 'Help' })
  expect(link).toHaveAttribute('href', '/help')
  expect(link).not.toHaveAttribute('type')
  await user.tab()
  expect(link).toHaveFocus()
  await user.keyboard('{Enter}')
  expect(activate).toHaveBeenCalledOnce()
})

it('toggles the switch with Space or its label and skips disabled and locked switches', async () => {
  const user = userEvent.setup()
  const change = vi.fn()
  render(
    <>
      <Switch label="Notifications" onCheckedChange={change} />
      <Switch label="Scheduled" defaultChecked disabled onCheckedChange={change} />
      <Switch label="Unavailable" disabled onCheckedChange={change} />
      <Switch label="Restricted" locked onCheckedChange={change} />
      <button>Continue</button>
    </>,
  )

  const enabled = screen.getByRole('switch', { name: 'Notifications' })
  expect(enabled).not.toBeChecked()
  await user.tab()
  expect(enabled).toHaveFocus()
  await user.keyboard(' ')
  expect(enabled).toBeChecked()
  expect(change).toHaveBeenLastCalledWith(true)
  await user.click(screen.getByText('Notifications'))
  expect(enabled).not.toBeChecked()
  expect(change).toHaveBeenLastCalledWith(false)
  expect(screen.getByRole('switch', { name: 'Scheduled' })).toBeChecked()
  for (const name of ['Scheduled', 'Unavailable', 'Restricted']) {
    expect(screen.getByRole('switch', { name })).toBeDisabled()
    await user.click(screen.getByText(name))
  }
  expect(screen.getByRole('switch', { name: 'Restricted' })).not.toBeChecked()
  expect(change).toHaveBeenCalledTimes(2)
  enabled.focus()
  await user.tab()
  expect(screen.getByRole('button', { name: 'Continue' })).toHaveFocus()
})

it('associates a field with its label, hint and error and supports typing by keyboard', async () => {
  const user = userEvent.setup()
  render(<Field label="Title" description="Use a short title." error="Title is required." />)
  const field = screen.getByRole('textbox', { name: 'Title' })
  expect(field).toHaveAttribute('aria-invalid', 'true')
  expect(field).toHaveAccessibleDescription('Use a short title. Title is required.')
  await user.tab()
  expect(field).toHaveFocus()
  await user.keyboard('Ethics')
  expect(field).toHaveValue('Ethics')
})

it('associates textarea errors and preserves line breaks typed by keyboard', async () => {
  const user = userEvent.setup()
  render(<Textarea label="Notes" error="Notes are required." />)
  const field = screen.getByRole('textbox', { name: 'Notes' })
  expect(field).toHaveAttribute('aria-invalid', 'true')
  expect(field).toHaveAccessibleDescription('Notes are required.')
  await user.tab()
  await user.keyboard('First{Enter}Second')
  expect(field).toHaveValue('First\nSecond')
})

it('selects an available option by keyboard and handles empty choices', async () => {
  const user = userEvent.setup()
  const change = vi.fn()
  render(
    <>
      <Select label="Classroom" placeholder="Choose one" description="Pick an open classroom."
        options={[{ value: 'closed', label: 'Closed', disabled: true }, { value: 'open', label: 'Open' }]}
        onValueChange={change} />
      <Select label="Empty" placeholder="No options" options={[]} />
    </>,
  )

  const select = screen.getByRole('combobox', { name: 'Classroom' })
  expect(select).toHaveAccessibleDescription('Pick an open classroom.')
  await user.tab()
  expect(select).toHaveFocus()
  await user.keyboard('{ArrowDown}')
  expect(await screen.findByRole('option', { name: 'Open' })).toHaveFocus()
  await user.keyboard('{Enter}')
  expect(change).toHaveBeenCalledWith('open')
  expect(select).toHaveFocus()
  expect(select).toHaveTextContent('Open')
  expect(screen.getByRole('combobox', { name: 'Empty' })).toBeDisabled()
})

it('marks the chosen option with a check and explains why an option is unavailable', async () => {
  const user = userEvent.setup()
  render(
    <Select label="Classroom" defaultValue="ethics"
      options={[
        { value: 'ethics', label: 'Ethics' },
        { value: 'logic', label: 'Logic' },
        { value: 'old', label: 'Old room', disabled: true, hint: 'Archived' },
      ]} />,
  )

  await user.tab()
  await user.keyboard('{ArrowDown}')
  const chosen = await screen.findByRole('option', { name: /Ethics/ })
  expect(chosen).toHaveAttribute('aria-selected', 'true')
  expect(chosen.querySelector('[data-vedute-check]')).not.toBeNull()
  expect(screen.getByRole('option', { name: /Logic/ }).querySelector('[data-vedute-check]')).toBeNull()

  const unavailable = screen.getByRole('option', { name: /Old room/ })
  expect(unavailable).toHaveAttribute('aria-disabled', 'true')
  expect(unavailable).toHaveTextContent('Archived')
})

it('moves among available tabs with the arrow keys and shows only the selected panel', async () => {
  const user = userEvent.setup()
  render(
    <Tabs defaultValue="active">
      <TabsList aria-label="Classrooms">
        <TabsTrigger value="active">Active</TabsTrigger>
        <TabsTrigger value="hidden" disabled>Hidden</TabsTrigger>
        <TabsTrigger value="archived">Archived</TabsTrigger>
      </TabsList>
      <TabsContent value="active">Active classrooms</TabsContent>
      <TabsContent value="archived">Archived classrooms</TabsContent>
    </Tabs>,
  )
  await user.tab()
  expect(screen.getByRole('tab', { name: 'Active' })).toHaveFocus()
  expect(screen.getByRole('tabpanel')).toHaveTextContent('Active classrooms')
  await user.keyboard('{ArrowRight}')
  expect(screen.getByRole('tab', { name: 'Archived', selected: true })).toHaveFocus()
  expect(screen.getByRole('tabpanel')).toHaveTextContent('Archived classrooms')
})

it('renders metadata and status as readable, non-interactive text', async () => {
  const user = userEvent.setup()
  render(
    <>
      <Meta icon={User} label="Author" value="Ada" />
      <StatusChip tone="active">Open</StatusChip>
      <StatusChip tone="draft">Draft</StatusChip>
      <StatusChip tone="inactive">Archived</StatusChip>
      <button>Continue</button>
    </>,
  )
  for (const text of ['Author', 'Ada', 'Open', 'Draft', 'Archived']) {
    expect(screen.getByText(text)).toBeVisible()
  }
  expect(screen.queryByRole('img')).not.toBeInTheDocument()
  await user.tab()
  expect(screen.getByRole('button', { name: 'Continue' })).toHaveFocus()
})

it('distinguishes informational and urgent page alerts without taking focus', async () => {
  const user = userEvent.setup()
  render(
    <>
      <Alert>Read-only access</Alert>
      <Alert kind="error">Loading failed <Button>Retry</Button></Alert>
    </>,
  )
  expect(screen.getByRole('status')).toHaveTextContent('Read-only access')
  expect(screen.getByRole('alert')).toHaveTextContent('Loading failed')
  await user.tab()
  expect(screen.getByRole('button', { name: 'Retry' })).toHaveFocus()
})

it('announces a busy skeleton region once and keeps placeholder blocks out of the tab order', async () => {
  const user = userEvent.setup()
  const { container } = render(
    <>
      <SkeletonRegion loadingLabel="Loading classrooms">
        <Skeleton className="h-6" />
        <Skeleton className="h-6" />
      </SkeletonRegion>
      <button>Continue</button>
    </>,
  )
  expect(screen.getByRole('status')).toHaveTextContent('Loading classrooms')
  expect(screen.getByRole('status').closest('[aria-busy]')).toBeNull()
  const busy = container.querySelector('[aria-busy="true"]')!
  expect(busy.querySelectorAll('[aria-hidden="true"]')).toHaveLength(2)
  await user.tab()
  expect(screen.getByRole('button', { name: 'Continue' })).toHaveFocus()
})

describe('Pagination', () => {
  const labels = { navigation: 'Pages', previous: 'Previous page', next: 'Next page', page: (page: number) => `Page ${page}` }

  it('supports keyboard page changes, current page semantics, and disabled boundaries', async () => {
    const user = userEvent.setup()
    const change = vi.fn()
    const { rerender } = render(<Pagination page={1} pageCount={3} labels={labels} onPageChange={change} />)
    const pages = within(screen.getByRole('navigation', { name: 'Pages' }))
    expect(pages.getByRole('button', { name: 'Previous page' })).toBeDisabled()
    expect(pages.getByRole('button', { name: 'Page 1' })).toHaveAttribute('aria-current', 'page')
    await user.tab()
    await user.keyboard('{Enter}')
    expect(change).not.toHaveBeenCalled()
    await user.tab()
    expect(pages.getByRole('button', { name: 'Page 2' })).toHaveFocus()
    await user.keyboard(' ')
    expect(change).toHaveBeenLastCalledWith(2)
    rerender(<Pagination page={3} pageCount={3} labels={labels} onPageChange={change} />)
    expect(pages.getByRole('button', { name: 'Next page' })).toBeDisabled()
    await user.click(pages.getByRole('button', { name: 'Previous page' }))
    expect(change).toHaveBeenLastCalledWith(2)
  })

  it('hides empty or single-page pagination and keeps a bounded page window', () => {
    const { rerender } = render(<Pagination page={1} pageCount={0} labels={labels} onPageChange={vi.fn()} />)
    expect(screen.queryByRole('navigation')).not.toBeInTheDocument()
    rerender(<Pagination page={1} pageCount={1} labels={labels} onPageChange={vi.fn()} />)
    expect(screen.queryByRole('navigation')).not.toBeInTheDocument()
    rerender(<Pagination page={50} pageCount={100} labels={labels} onPageChange={vi.fn()} />)
    expect(screen.getAllByRole('button')).toHaveLength(7)
    expect(screen.getByRole('button', { name: 'Page 50', current: 'page' })).toBeVisible()
  })
})

describe('Toast', () => {
  afterEach(() => vi.useRealTimers())

  it.each(['success', 'info'] as const)('announces %s and dismisses it after four seconds', (kind) => {
    vi.useFakeTimers()
    const dismiss = vi.fn()
    render(<Toast notification={{ id: 1, kind, message: 'Settings saved' }} dismissLabel="Dismiss" onDismiss={dismiss} />)
    expect(screen.getByRole('status')).toHaveTextContent('Settings saved')
    act(() => vi.advanceTimersByTime(3999))
    expect(dismiss).not.toHaveBeenCalled()
    act(() => vi.advanceTimersByTime(1))
    expect(dismiss).toHaveBeenCalledOnce()
    expect(dismiss).toHaveBeenCalledWith(1)
  })

  it('pauses while hovered or focused, then resumes the same timer', () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'performance'] })
    const dismiss = vi.fn()
    render(<Toast notification={{ id: 1, kind: 'success', message: 'Saved' }} dismissLabel="Dismiss" onDismiss={dismiss} />)
    const close = screen.getByRole('button', { name: 'Dismiss' })
    act(() => vi.advanceTimersByTime(1000))
    fireEvent.mouseOver(close)
    act(() => vi.advanceTimersByTime(5000))
    act(() => close.focus())
    expect(close).toHaveFocus()
    fireEvent.mouseOut(close)
    act(() => vi.advanceTimersByTime(5000))
    expect(dismiss).not.toHaveBeenCalled()
    act(() => close.blur())
    act(() => vi.advanceTimersByTime(2999))
    expect(dismiss).not.toHaveBeenCalled()
    act(() => vi.advanceTimersByTime(1))
    expect(dismiss).toHaveBeenCalledWith(1)
  })

  it('replaces the previous toast and keeps errors until a keyboard dismissal', async () => {
    vi.useFakeTimers()
    const dismiss = vi.fn()
    const retry = vi.fn()
    const { container, rerender } = render(
      <Toast notification={{ id: 1, kind: 'info', message: 'Copying' }} dismissLabel="Dismiss" onDismiss={dismiss} />,
    )
    rerender(
      <Toast notification={{ id: 3, kind: 'info', message: 'Trying again' }} dismissLabel="Dismiss" onDismiss={dismiss} />,
    )
    rerender(
      <Toast notification={{ id: 2, kind: 'error', message: 'Copy failed', action: { label: 'Retry', onClick: retry } }}
        dismissLabel="Dismiss" onDismiss={dismiss} />,
    )
    const leaving = container.querySelector('.vedute-toast')!
    expect(leaving).toHaveAttribute('inert')
    expect(leaving).toHaveTextContent('Copying')
    expect(screen.getByRole('alert')).toBeEmptyDOMElement()
    const animationEnd = 'AnimationEvent' in window ? 'animationend' : 'webkitAnimationEnd'
    fireEvent(leaving, Object.assign(new Event(animationEnd, { bubbles: true }), {
      animationName: 'vedute-toast-exit',
    }))
    expect(screen.queryByText('Copying')).not.toBeInTheDocument()
    expect(screen.queryByText('Trying again')).not.toBeInTheDocument()
    expect(screen.getByRole('alert')).toHaveTextContent('Copy failed')
    act(() => vi.advanceTimersByTime(8000))
    expect(dismiss).not.toHaveBeenCalled()
    vi.useRealTimers()
    const user = userEvent.setup()
    await user.tab()
    expect(screen.getByRole('button', { name: 'Retry' })).toHaveFocus()
    await user.keyboard('{Enter}')
    expect(retry).toHaveBeenCalledOnce()
    await user.tab()
    expect(screen.getByRole('button', { name: 'Dismiss' })).toHaveFocus()
    await user.keyboard('{Enter}')
    expect(dismiss).toHaveBeenCalledWith(2)
  })
})
