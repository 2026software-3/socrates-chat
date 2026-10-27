import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
import { useAuth } from '@/auth/auth-context'
import { apiError, makeMe, renderApp } from '@/test/render'
import { server } from '@/test/server'

describe('language picked before logging in', () => {
  // 伺服器在帳號第一次登入時依 Accept-Language 決定語言（這裡是 en）；
  // 使用者先在登入頁選了 Español，登入後不能被改回 en
  async function loginAfterPicking(serverLanguage: 'en' | 'es') {
    let loggedIn = false
    const patches: unknown[] = []
    server.use(
      http.get('/api/me', () =>
        loggedIn ? HttpResponse.json(makeMe({ student: true }, { language: serverLanguage })) : apiError(401, 'unauthorized'),
      ),
      http.post('/api/auth/login', () => {
        loggedIn = true
        return HttpResponse.json({ must_change_password: false })
      }),
      http.patch('/api/me', async ({ request }) => {
        const body = (await request.json()) as { language: 'en' | 'es' }
        patches.push(body)
        serverLanguage = body.language
        return HttpResponse.json(makeMe({ student: true }, { language: body.language }))
      }),
    )
    renderApp(null, { route: '/login' })
    await userEvent.selectOptions(await screen.findByLabelText('語言'), 'es')
    await userEvent.type(await screen.findByLabelText('Correo electrónico'), 'a@example.com')
    await userEvent.type(screen.getByLabelText('Contraseña'), 'password-123')
    await userEvent.click(screen.getByRole('button', { name: 'Iniciar sesión' }))
    await screen.findByRole('navigation', { name: 'main' })
    return patches
  }

  it('keeps the chosen language and saves it to the account', async () => {
    const patches = await loginAfterPicking('en')
    await waitFor(() => expect(patches).toEqual([{ language: 'es' }]))
    expect(document.documentElement.lang).toBe('es')
  })

  it('does not patch when the server already agrees', async () => {
    const patches = await loginAfterPicking('es')
    expect(patches).toEqual([])
    expect(document.documentElement.lang).toBe('es')
  })
})

describe('a leftover pre-login pick must not reach the next account', () => {
  const KEY = 'lang-picked-before-login'
  const stored = (lang: string, ageMs: number) => JSON.stringify({ lang, at: Date.now() - ageMs })

  function trackPatches(me: ReturnType<typeof makeMe> | null) {
    const patches: unknown[] = []
    server.use(
      http.get('/api/me', () => (me ? HttpResponse.json(me) : apiError(401, 'unauthorized'))),
      http.patch('/api/me', async ({ request }) => {
        patches.push(await request.json())
        return HttpResponse.json(me)
      }),
    )
    return patches
  }

  it('ignores a pick older than ten minutes', async () => {
    sessionStorage.setItem(KEY, stored('es', 11 * 60_000))
    const patches = trackPatches(makeMe({ student: true }, { language: 'en' }))
    renderApp(null, { route: '/' })
    await screen.findByRole('navigation', { name: 'main' })
    expect(patches).toEqual([])
    expect(sessionStorage.getItem(KEY)).toBeNull()
  })

  it('still applies a recent pick, as after the Google redirect', async () => {
    sessionStorage.setItem(KEY, stored('es', 30_000))
    const patches = trackPatches(makeMe({ student: true }, { language: 'en' }))
    renderApp(null, { route: '/' })
    await waitFor(() => expect(patches).toEqual([{ language: 'es' }]))
  })

  it('drops a pick left from before this page load when nobody is signed in', async () => {
    sessionStorage.setItem(KEY, stored('es', 60_000))
    trackPatches(null)
    renderApp(null, { route: '/login' })
    await screen.findByLabelText('電子郵件')
    await waitFor(() => expect(sessionStorage.getItem(KEY)).toBeNull())
  })

  it('drops the pick on logout', async () => {
    trackPatches(makeMe({ student: true }))
    function LogoutButton() {
      const auth = useAuth()
      return <button onClick={() => void auth.logout()}>bye</button>
    }
    server.use(http.post('/api/auth/logout', () => new HttpResponse(null, { status: 204 })))
    renderApp(<LogoutButton />, { route: '/' })
    await userEvent.click(await screen.findByRole('button', { name: 'bye' }))
    sessionStorage.setItem(KEY, stored('es', 1_000))
    await userEvent.click(screen.getByRole('button', { name: 'bye' }))
    await waitFor(() => expect(sessionStorage.getItem(KEY)).toBeNull())
  })
})
