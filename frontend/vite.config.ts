import path from 'node:path'
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import type { Plugin } from 'vite'
import { defineConfig } from 'vitest/config'

/**
 * `--mode e2e`（Playwright）時由 dev server 提供 MSW 的 service worker 腳本。
 * 直接讀 node_modules 裡的檔案，版本永遠與 msw 套件一致，也不必放進 public/（正式建置不會帶到）。
 */
function mswServiceWorker(): Plugin {
  const file = createRequire(import.meta.url).resolve('msw/mockServiceWorker.js')
  return {
    name: 'msw-service-worker',
    apply: 'serve',
    configureServer(server) {
      server.middlewares.use('/mockServiceWorker.js', (_req, res) => {
        res.setHeader('Content-Type', 'text/javascript')
        res.end(readFileSync(file))
      })
    },
  }
}

// https://vite.dev/config/
export default defineConfig(({ mode }) => ({
  plugins: [react(), tailwindcss(), mode === 'e2e' && mswServiceWorker()],
  resolve: {
    alias: { '@': path.resolve(import.meta.dirname, './src') },
  },
  server: {
    // 前端與 API 同網域部署（S-08.1）；開發時把 /api 轉給本機的 Rust 後端
    proxy: { '/api': { target: 'http://127.0.0.1:3000', ws: true } },
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.ts'],
    // tests/e2e/ 是 Playwright（npm run test:e2e），不歸 Vitest 跑
    include: ['src/**/*.test.{ts,tsx}'],
    css: false,
    // 要比 testing-library 的 asyncUtilTimeout（src/test/setup.ts，5 秒）長，否則等不到就先被單一測試逾時截斷
    testTimeout: 15_000,
    // 固定時區，避免日期斷言隨機器時區而變
    env: { TZ: 'UTC' },
  },
}))
