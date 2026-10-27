import { screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { http, HttpResponse } from 'msw'
import { describe, expect, it } from 'vitest'
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
