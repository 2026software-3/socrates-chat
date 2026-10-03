import { defineMessages } from '@/i18n/define'

export const authMessages = defineMessages({
  'zh-TW': {
    'login.title': '登入',
    'login.subtitle': '使用 Google 帳號登入，開始思辨討論。',
    'login.google': '使用 Google 登入',
    'login.error.invalid_state': '登入已逾時或無效，請再試一次。',
    'login.error.email_not_verified': '你的 Google 電子郵件尚未驗證。',
    'login.error.access_denied': '你取消了授權，尚未登入。',
    'login.error.login_failed': '登入失敗，請稍後再試。',
    'pending.title': '尚未開通',
    'pending.body': '你的帳號還沒有加入修課名單。請聯絡授課教師，加入後重新整理即可使用。',
    'pending.email': '目前登入的帳號：{email}',
  },
  en: {
    'login.title': 'Log in',
    'login.subtitle': 'Sign in with your Google account to start a discussion.',
    'login.google': 'Sign in with Google',
    'login.error.invalid_state': 'The login expired or was invalid. Please try again.',
    'login.error.email_not_verified': 'Your Google email address is not verified.',
    'login.error.access_denied': 'You cancelled the authorization, so you are not logged in.',
    'login.error.login_failed': 'Login failed. Please try again later.',
    'pending.title': 'Not enabled yet',
    'pending.body':
      'Your account is not on the course roster yet. Ask your teacher to add you, then reload this page.',
    'pending.email': 'Signed in as: {email}',
  },
  es: {
    'login.title': 'Iniciar sesión',
    'login.subtitle': 'Entra con tu cuenta de Google para empezar una discusión.',
    'login.google': 'Entrar con Google',
    'login.error.invalid_state': 'El inicio de sesión caducó o no era válido. Inténtalo de nuevo.',
    'login.error.email_not_verified': 'Tu correo de Google no está verificado.',
    'login.error.access_denied': 'Cancelaste la autorización, así que no has iniciado sesión.',
    'login.error.login_failed': 'No se pudo iniciar sesión. Inténtalo más tarde.',
    'pending.title': 'Aún no habilitado',
    'pending.body':
      'Tu cuenta todavía no está en la lista del curso. Pide al docente que te añada y recarga la página.',
    'pending.email': 'Sesión iniciada como: {email}',
  },
})
