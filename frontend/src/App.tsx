import type { ReactNode } from 'react'
import { BrowserRouter } from 'react-router'
import { AppRoutes } from '@/app/routes'
import { AuthProvider, useAuth } from '@/auth/auth-context'
import { Toaster } from '@/components/ui/sonner'
import { TooltipProvider } from '@/components/ui/tooltip'
import { I18nProvider } from '@/i18n'
import { AiJobsProvider } from '@/lib/ai-jobs'

function JobsScope({ children }: { children: ReactNode }) {
  const auth = useAuth()
  return <AiJobsProvider userKey={auth.status === 'authed' ? auth.me.id : undefined}>{children}</AiJobsProvider>
}

export default function App() {
  return (
    <I18nProvider>
      <AuthProvider>
        <TooltipProvider>
          <JobsScope>
            <BrowserRouter>
              <AppRoutes />
            </BrowserRouter>
          </JobsScope>
          <Toaster />
        </TooltipProvider>
      </AuthProvider>
    </I18nProvider>
  )
}
