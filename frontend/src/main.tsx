import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'

/** Playwright（`vite --mode e2e`）以瀏覽器內的 MSW 代替後端；其他模式這段在建置時就被移除。 */
async function startMockBackend() {
  if (import.meta.env.MODE !== 'e2e') return
  const { startWorker } = await import('@/test/browser')
  await startWorker()
}

void startMockBackend().then(() => {
  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  )
})
