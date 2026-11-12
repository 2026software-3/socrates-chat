// 啟動一整套真實環境：全新的 PostgreSQL 資料庫 + 真正的後端執行檔 + 假的 OpenAI 伺服器。
// 每個測試檔各自啟動一套，彼此完全隔離，可以平行執行。
//
// 環境變數：
//   E2E_ADMIN_DATABASE_URL  能建立資料庫的連線（預設 CI／docker compose 的帳號，連到 postgres 資料庫）
//   E2E_BACKEND_BIN         後端執行檔（預設 backend/target/debug/socrates-chat-backend）
//   PSQL                    psql 路徑（預設從 PATH 找）
import { spawn, execFileSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { existsSync, mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import net from 'node:net';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Client } from './client.mjs';
import { startFakeOpenAi } from './fake-openai.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const ADMIN_DB_URL = process.env.E2E_ADMIN_DATABASE_URL ?? 'postgres://socrates:socrates@127.0.0.1:5432/postgres';
const BACKEND_BIN = process.env.E2E_BACKEND_BIN ?? path.resolve(here, '../../backend/target/debug/socrates-chat-backend');
const PSQL = process.env.PSQL ?? 'psql';

export const ADMIN_EMAIL = 'admin@e2e.test';
export const ADMIN2_EMAIL = 'admin2@e2e.test';
export const ADMIN_INITIAL_PASSWORD = 'Init-Admin-Pass-1';

export function psql(url, sql) {
  return execFileSync(PSQL, ['-X', '-A', '-t', '-q', '-v', 'ON_ERROR_STOP=1', url, '-c', sql], { encoding: 'utf8' }).trim();
}

function withDatabase(url, name) {
  const u = new URL(url);
  u.pathname = `/${name}`;
  return u.toString();
}

function freePort() {
  return new Promise((resolve, reject) => {
    const s = net.createServer();
    s.once('error', reject);
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export async function until(fn, { timeout = 10000, every = 50, what = 'condition' } = {}) {
  const end = Date.now() + timeout;
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await sleep(every);
  }
}

let counter = 0;
export const uniq = (prefix = 'u') => `${prefix}${Date.now().toString(36)}${(counter++).toString(36)}${randomBytes(2).toString('hex')}`;

/**
 * @param {object} [opts]
 * @param {Record<string,string>} [opts.env] 覆寫／追加後端環境變數；值為 null 代表移除該變數
 * @param {boolean} [opts.speech] 設定語音備援模型（預設 true）
 * @param {boolean} [opts.frontend] 提供假的前端建置產物（預設 false）
 */
export async function startStack(opts = {}) {
  if (!existsSync(BACKEND_BIN)) {
    throw new Error(`backend binary not found: ${BACKEND_BIN} (run "cargo build" in backend/ or set E2E_BACKEND_BIN)`);
  }
  const dbName = `e2e_${randomBytes(6).toString('hex')}`;
  psql(ADMIN_DB_URL, `CREATE DATABASE ${dbName}`);
  const dbUrl = withDatabase(ADMIN_DB_URL, dbName);
  const fake = await startFakeOpenAi();
  const port = await freePort();
  const base = `http://127.0.0.1:${port}`;

  let frontendDir;
  if (opts.frontend) {
    frontendDir = mkdtempSync(path.join(tmpdir(), 'e2e-fe-'));
    writeFileSync(path.join(frontendDir, 'index.html'), '<!doctype html><title>socrates-e2e-index</title><div id="root"></div>');
    writeFileSync(path.join(frontendDir, 'app.js'), 'console.log("e2e")');
  }

  const env = {
    PATH: process.env.PATH,
    DATABASE_URL: dbUrl,
    RUN_MIGRATIONS: 'true',
    APP_BASE_URL: base,
    LISTEN_ADDR: `127.0.0.1:${port}`,
    GOOGLE_CLIENT_ID: 'e2e-client-id',
    GOOGLE_CLIENT_SECRET: 'e2e-client-secret',
    ADMIN_EMAILS: `${ADMIN_EMAIL},${ADMIN2_EMAIL}`,
    ADMIN_INITIAL_PASSWORD,
    COOKIE_SECURE: 'false',
    OPENAI_API_KEY: fake.apiKey,
    OPENAI_MODEL: 'e2e-model',
    OPENAI_BASE_URL: fake.url,
    WRAP_UP_TURN: '3',
    MAX_TURNS: '5',
    AI_FIRST_TOKEN_TIMEOUT_SECS: '1',
    AI_TOTAL_TIMEOUT_SECS: '3',
    RUST_LOG: 'info',
    ...(opts.speech === false ? {} : { OPENAI_STT_MODEL: 'e2e-stt', OPENAI_TTS_MODEL: 'e2e-tts' }),
    ...(frontendDir ? { FRONTEND_DIR: frontendDir } : {}),
  };
  for (const [k, v] of Object.entries(opts.env ?? {})) {
    if (v === null) delete env[k];
    else env[k] = v;
  }

  let logs = '';
  const child = spawn(BACKEND_BIN, [], { env, stdio: ['ignore', 'pipe', 'pipe'] });
  child.stdout.on('data', (d) => { logs += d; });
  child.stderr.on('data', (d) => { logs += d; });
  let exited = false;
  child.on('exit', () => { exited = true; });

  const cleanup = async () => {
    if (!exited) {
      child.kill('SIGTERM');
      await until(() => exited, { timeout: 5000, what: 'backend exit' }).catch(() => child.kill('SIGKILL'));
    }
    await fake.close();
    try { psql(ADMIN_DB_URL, `DROP DATABASE IF EXISTS ${dbName} WITH (FORCE)`); } catch { /* best effort */ }
    if (frontendDir) rmSync(frontendDir, { recursive: true, force: true });
  };

  try {
    await until(async () => {
      if (exited) throw new Error(`backend exited early:\n${logs}`);
      try { return (await fetch(`${base}/health`)).ok; } catch { return false; }
    }, { timeout: 60000, every: 100, what: 'backend /health' });
  } catch (e) {
    await cleanup();
    throw e;
  }

  const stack = {
    base,
    dbUrl,
    fake,
    env,
    frontendDir,
    logs: () => logs,
    sql: (q) => psql(dbUrl, q),
    client: (o) => new Client(base, o),
    stop: cleanup,
  };
  Object.assign(stack, actors(stack));
  return stack;
}

function actors(stack) {
  const api = {};

  /** 以密碼登入並（若需要）更改成新密碼；回傳已登入的 client 與最終密碼。 */
  api.signIn = async (email, password, { keepTemporary = false } = {}) => {
    const c = stack.client();
    const r = await c.post('/api/auth/login', { email, password });
    if (r.status !== 200) throw new Error(`login ${email} failed: ${r.status} ${r.text}`);
    let finalPassword = password;
    if (r.json.must_change_password && !keepTemporary) {
      finalPassword = `Pw-${randomBytes(6).toString('hex')}`;
      const ch = await c.post('/api/auth/change-password', { current_password: password, new_password: finalPassword });
      if (ch.status !== 204) throw new Error(`change-password failed: ${ch.status} ${ch.text}`);
    }
    return Object.assign(c, { email, password: finalPassword });
  };

  let adminPromise;
  /** 首位管理者（ADMIN_EMAILS 指定，用 ADMIN_INITIAL_PASSWORD 登入並改密碼）。每個 stack 只做一次。 */
  api.admin = () => (adminPromise ??= api.signIn(ADMIN_EMAIL, ADMIN_INITIAL_PASSWORD));

  /** 管理者新增教師並以重設密碼建立帳號，回傳已登入的教師 client。 */
  api.teacher = async (email = `${uniq('teacher')}@e2e.test`) => {
    const admin = await api.admin();
    const add = await admin.post('/api/admin/teachers', { email });
    if (add.status !== 204) throw new Error(`add teacher failed: ${add.status} ${add.text}`);
    const reset = await admin.post('/api/admin/users/reset-password', { email });
    if (reset.status !== 200) throw new Error(`reset failed: ${reset.status} ${reset.text}`);
    return api.signIn(email, reset.json.temporary_password);
  };

  /** 管理者匯入修課名單並用自動產生的臨時密碼登入，回傳已登入的學生 client。 */
  api.student = async (email = `${uniq('student')}@e2e.test`) => {
    const admin = await api.admin();
    const imp = await admin.post('/api/roster/import', { text: email });
    if (imp.status !== 200) throw new Error(`import failed: ${imp.status} ${imp.text}`);
    const cred = imp.json.credentials.find((c) => c.email === email);
    if (!cred) throw new Error(`no credential issued for ${email}: ${imp.text}`);
    return api.signIn(email, cred.temporary_password);
  };

  /** 學生開始一場對話（預設電車難題）。 */
  api.startConversation = async (student, source) => {
    let body = source;
    if (!body) {
      const avail = await student.get('/api/available');
      const trolley = avail.json.topics.find((t) => t.title === '電車難題');
      body = { topic_id: trolley.id };
    }
    const r = await student.post('/api/conversations', body);
    if (r.status !== 201) throw new Error(`create conversation failed: ${r.status} ${r.text}`);
    return r.json;
  };

  /** 送出一則學生訊息並讀完 AI 回覆串流。 */
  api.turn = async (student, convId, content, { source } = {}) => {
    const m = await student.post(`/api/conversations/${convId}/messages`, { content, ...(source ? { source } : {}) });
    if (m.status !== 201) return { message: m };
    const s = await student.sse(`/api/conversations/${convId}/stream`);
    return { message: m, stream: s };
  };

  /** 輪詢到總結進入指定狀態。 */
  api.waitSummary = (student, convId, status = 'ready') =>
    until(async () => {
      const r = await student.get(`/api/conversations/${convId}/summary`);
      return r.json?.status === status ? r.json : false;
    }, { timeout: 15000, what: `summary ${status}` });

  /** 完成一場對話：開始、來回 n 輪、結束、等總結。回傳 { conv, summary }。 */
  api.completeConversation = async (student, { turns = 1, source } = {}) => {
    const conv = await api.startConversation(student, source);
    for (let i = 0; i < turns; i++) {
      const t = await api.turn(student, conv.id, `我的看法 ${i + 1}`);
      if (t.stream?.events.at(-1)?.event !== 'done') throw new Error(`turn ${i + 1} did not finish: ${JSON.stringify(t)}`);
    }
    const end = await student.post(`/api/conversations/${conv.id}/end`);
    if (![200, 202].includes(end.status)) throw new Error(`end failed: ${end.status} ${end.text}`);
    const summary = await api.waitSummary(student, conv.id);
    return { conv, summary };
  };

  return api;
}
