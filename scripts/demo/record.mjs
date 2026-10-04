// 錄製 README 用的操作演示影片：學生與 AI 討論 → 產生總結 → 教師看班上分布。
// 前置：服務已啟動並跑過 scripts/seed-demo.sh（使用其示範帳號，全部是合成資料）。
// 學生對話會真的呼叫 AI，會產生少量費用，每次 AI 回覆內容也不同。
// 用法（在 scripts/demo/）：
//   npm install && npx playwright install chromium
//   npm run record
// 可用環境變數：
//   BASE_URL     前端網址，預設 http://localhost:5173
//   CHROME_PATH  改用指定的 Chrome / Chromium 執行檔（Playwright 無法下載瀏覽器時使用）
//   OUT_DIR      輸出目錄，預設 ../../docs/demo
import { execFileSync } from 'node:child_process'
import { mkdirSync, readdirSync, renameSync, rmSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright'

const here = dirname(fileURLToPath(import.meta.url))
const BASE_URL = process.env.BASE_URL ?? 'http://localhost:5173'
const OUT_DIR = resolve(process.env.OUT_DIR ?? join(here, '../../docs/demo'))
const RAW_DIR = join(here, '.raw')
const SIZE = { width: 1280, height: 800 }

const STUDENT = { email: 'student1@example.com', password: 'Student-Demo-2026' }
const TEACHER = { email: 'teacher@example.com', password: 'Teacher-Demo-2026' }
const TURNS = [
  '我會拉下拉桿，因為犧牲一個人救五個人，整體損失比較小。',
  '我想每條生命的價值應該是一樣的，所以人數多的一邊比較重要。',
  '如果是要把一個人推下橋來擋電車，我就不會做，因為那是我親手殺人。',
]

// 影片看不到滑鼠，注入一個跟著滑鼠移動的圓點，讓觀眾知道點了哪裡
const cursorScript = () => {
  window.addEventListener('DOMContentLoaded', () => {
    const dot = document.createElement('div')
    dot.style.cssText =
      'position:fixed;z-index:2147483647;width:18px;height:18px;margin:-9px 0 0 -9px;border-radius:50%;' +
      'background:rgba(239,68,68,.55);border:2px solid #fff;pointer-events:none;left:-40px;top:-40px;transition:transform .1s'
    document.body.appendChild(dot)
    document.addEventListener('mousemove', (e) => {
      dot.style.left = `${e.clientX}px`
      dot.style.top = `${e.clientY}px`
    })
    document.addEventListener('mousedown', () => (dot.style.transform = 'scale(.7)'))
    document.addEventListener('mouseup', () => (dot.style.transform = ''))
  })
}

const pause = (page, ms) => page.waitForTimeout(ms)

/** 移動滑鼠到元素中央再點，讓影片看得到游標移動。 */
async function click(page, locator) {
  await locator.scrollIntoViewIfNeeded()
  const box = await locator.boundingBox()
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 15 })
  await pause(page, 250)
  await locator.click()
}

async function type(page, locator, text) {
  await click(page, locator)
  await locator.pressSequentially(text, { delay: 60 })
}

/** 平滑捲動到頁面底部。 */
async function scrollDown(page, step = 250) {
  for (;;) {
    const done = await page.evaluate((s) => {
      window.scrollBy({ top: s, behavior: 'smooth' })
      return window.innerHeight + window.scrollY >= document.body.scrollHeight - 4
    }, step)
    await pause(page, 400)
    if (done) break
  }
}

async function login(page, { email, password }) {
  await page.goto(`${BASE_URL}/login`)
  await pause(page, 800)
  await type(page, page.getByLabel('電子郵件'), email)
  await type(page, page.getByLabel('密碼'), password)
  await click(page, page.getByRole('button', { name: '登入', exact: true }))
  await page.waitForURL((url) => !url.pathname.startsWith('/login'))
  await pause(page, 1500)
}

async function logout(page) {
  await click(page, page.getByRole('button', { name: '登出' }))
  await page.waitForURL('**/login')
  await pause(page, 800)
}

async function studentFlow(page) {
  await login(page, STUDENT)
  await click(page, page.getByRole('button', { name: '開始討論：電車難題' }))
  await page.waitForURL('**/conversations/*')
  await pause(page, 1500)

  const input = page.getByRole('textbox', { name: '訊息輸入' })
  const aiMessages = page.locator('[data-role="ai"]')
  for (const text of TURNS) {
    const before = await aiMessages.count()
    await type(page, input, text)
    await pause(page, 400)
    await click(page, page.getByRole('button', { name: '送出' }))
    // 等 AI 串流回覆完成：出現新的 AI 訊息，且輸入框重新可用
    await aiMessages.nth(before).waitFor({ timeout: 60_000 })
    await page.waitForFunction(
      () => !document.querySelector('[data-role="ai"] [role="status"]'),
      undefined,
      { timeout: 60_000 },
    )
    await pause(page, 3000)
  }

  await click(page, page.getByRole('button', { name: '結束討論' }))
  await pause(page, 1200)
  await click(page, page.getByRole('button', { name: '結束並產生總結' }))
  // 總結區塊會先顯示「正在產生總結…」，等最終主張出現才算產生完成
  await page.getByText('AI 學習總結').waitFor({ timeout: 30_000 })
  await page.getByText('AI 學習總結').scrollIntoViewIfNeeded()
  await page.getByText('最終主張').waitFor({ timeout: 120_000 })
  await pause(page, 1500)
  await scrollDown(page)
  await pause(page, 2500)
  await logout(page)
}

async function teacherFlow(page) {
  await login(page, TEACHER)
  if (!page.url().includes('/teacher/dashboard')) await page.goto(`${BASE_URL}/teacher/dashboard`)
  await pause(page, 2500)
  await scrollDown(page, 200)
  await pause(page, 2500)
}

function convert(webm) {
  mkdirSync(OUT_DIR, { recursive: true })
  const mp4 = join(OUT_DIR, 'demo.mp4')
  const gif = join(OUT_DIR, 'demo.gif')
  const ff = (args) => execFileSync('ffmpeg', ['-y', '-loglevel', 'error', ...args], { stdio: 'inherit' })
  ff(['-i', webm, '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-crf', '26', '-movflags', '+faststart', mp4])
  // README 內嵌用的 GIF：加速 1.5 倍、縮小、調色盤降低檔案大小
  ff([
    '-i', webm,
    '-vf', 'setpts=PTS/1.5,fps=10,scale=960:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128[p];[b][p]paletteuse=dither=bayer:bayer_scale=5',
    gif,
  ])
  console.log(`輸出：\n  ${mp4}\n  ${gif}`)
}

rmSync(RAW_DIR, { recursive: true, force: true })
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || undefined })
const context = await browser.newContext({
  viewport: SIZE,
  locale: 'zh-TW',
  recordVideo: { dir: RAW_DIR, size: SIZE },
})
await context.addInitScript(cursorScript)
const page = await context.newPage()
try {
  await studentFlow(page)
  await teacherFlow(page)
} finally {
  await context.close()
  await browser.close()
}
const webm = join(RAW_DIR, readdirSync(RAW_DIR).find((f) => f.endsWith('.webm')))
renameSync(webm, join(RAW_DIR, 'demo.webm'))
convert(join(RAW_DIR, 'demo.webm'))
rmSync(RAW_DIR, { recursive: true, force: true })
