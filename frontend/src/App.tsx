import { BrowserRouter } from 'react-router'
import { AppRoutes } from '@/app/routes'
import { AuthProvider } from '@/auth/auth-context'
import { Toaster } from '@/components/ui/sonner'
import { TooltipProvider } from '@/components/ui/tooltip'
import { I18nProvider } from '@/i18n'

export default function App() {
  return (
    <I18nProvider>
      <AuthProvider>
        <TooltipProvider>
          <BrowserRouter>
            <AppRoutes />
          </BrowserRouter>
          <Toaster />
        </TooltipProvider>
      </AuthProvider>
    </I18nProvider>
  )
}
