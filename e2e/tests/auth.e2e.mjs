// S-01.1、S-01.2、S-08.2：內建登入、密碼規則、登入限流、session、CSRF、Google 登入入口。
import { describe, test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { startStack, uniq, ADMIN_EMAIL, ADMIN2_EMAIL, ADMIN_INITIAL_PASSWORD } from '../lib/stack.mjs';

let s;
before(async () => { s = await startStack(); });
after(async () => { await s?.stop(); });

const sha256hex = (v) => createHash('sha256').update(v).digest('hex');

describe('S-08.2 §1 內建登入', () => {
  test('首位管理者以 ADMIN_INITIAL_PASSWORD 登入：建立 session、必須更改密碼', async () => {
    const c = s.client();
    const r = await c.post('/api/auth/login', { email: ADMIN_EMAIL, password: ADMIN_INITIAL_PASSWORD });
    assert.equal(r.status, 200);
    assert.deepEqual(r.json, { must_change_password: true });
    assert.ok(c.sid, '應設定 sid cookie');

    const me = await c.get('/api/me');
    assert.equal(me.status, 200);
    assert.equal(me.json.email, ADMIN_EMAIL);
    assert.equal(me.json.is_admin, true);
    assert.equal(me.json.must_change_password, true);
    assert.equal(me.json.roles.admin, true);
  });

  test('ADMIN_EMAILS 中的每個信箱都會建立內建管理者帳號', async () => {
    const c = s.client();
    const r = await c.post('/api/auth/login', { email: ADMIN2_EMAIL, password: ADMIN_INITIAL_PASSWORD });
    assert.equal(r.status, 200);
    assert.equal((await c.get('/api/me')).json.is_admin, true);
  });

  test('電子郵件不分大小寫、會去除前後空白（S-01.2）', async () => {
    const c = s.client();
    const r = await c.post('/api/auth/login', { email: `  ${ADMIN_EMAIL.toUpperCase()} `, password: ADMIN_INITIAL_PASSWORD });
    assert.equal(r.status, 200);
    assert.equal((await c.get('/api/me')).json.email, ADMIN_EMAIL);
  });

  test('密碼以 Argon2id 保存，不是明文（S-01.1）', () => {
    const hash = s.sql(`SELECT password_hash FROM users WHERE email = '${ADMIN_EMAIL}'`);
    assert.match(hash, /^\$argon2id\$/);
    assert.ok(!hash.includes(ADMIN_INITIAL_PASSWORD));
  });

  test('session cookie：HttpOnly、SameSite=Lax、Path=/；資料庫只存 SHA-256 雜湊（S-08.2 §4）', async () => {
    const c = s.client();
    const r = await c.post('/api/auth/login', { email: ADMIN_EMAIL, password: ADMIN_INITIAL_PASSWORD });
    const line = r.setCookie.find((l) => l.startsWith('sid='));
    assert.ok(line, '應該有 sid cookie');
    assert.match(line, /HttpOnly/i);
    assert.match(line, /SameSite=Lax/i);
    assert.match(line, /Path=\//i);
    assert.match(line, /Max-Age=2592000/i, '絕對上限 30 天');
    assert.match(c.sid, /^[A-Za-z0-9_-]{43}$/, '256 位元隨機值（base64url）');
    assert.equal(s.sql(`SELECT count(*) FROM sessions WHERE encode(token_hash, 'hex') = '${sha256hex(c.sid)}'`), '1');
    const raw = Buffer.from(c.sid, 'base64url').toString('hex');
    assert.equal(s.sql(`SELECT count(*) FROM sessions WHERE encode(token_hash, 'hex') = '${raw}'`), '0', '不存 token 原值');
  });

  test('沒有註冊端點（S-01.1）', async () => {
    const c = s.client();
    for (const p of ['/api/auth/register', '/api/register', '/api/auth/signup', '/api/users']) {
      const r = await c.post(p, { email: 'new@e2e.test', password: 'whatever-123' });
      assert.equal(r.status, 404, p);
    }
  });

  test('帳號不存在與密碼錯誤的回應相同：invalid_credentials（S-08.2）', async () => {
    const c = s.client();
    const wrongPw = await c.post('/api/auth/login', { email: ADMIN_EMAIL, password: 'definitely-wrong' });
    const noUser = await c.post('/api/auth/login', { email: `${uniq('nobody')}@e2e.test`, password: 'definitely-wrong' });
    assert.equal(wrongPw.status, 401);
    assert.equal(noUser.status, 401);
    assert.equal(wrongPw.json.error.code, 'invalid_credentials');
    assert.equal(noUser.json.error.code, 'invalid_credentials');
    assert.equal(wrongPw.json.error.request_id.length > 0, true);
    assert.ok(!c.sid, '失敗時不建立 session');
  });

  test('只用過 Google、沒有密碼的帳號用內建登入 → invalid_credentials', async () => {
    const email = `${uniq('gonly')}@e2e.test`;
    s.sql(`INSERT INTO users (email, google_sub) VALUES ('${email}', 'sub-${uniq()}')`);
    const r = await s.client().post('/api/auth/login', { email, password: 'any-password-1' });
    assert.equal(r.status, 401);
    assert.equal(r.json.error.code, 'invalid_credentials');
  });
});

describe('S-08.2 §1 臨時密碼與更改密碼', () => {
  test('臨時密碼 session 只能用 /api/me、改密碼、登出；其他 API 回 403 password_change_required', async () => {
    const teacherEmail = `${uniq('t')}@e2e.test`;
    const admin = await s.admin();
    await admin.post('/api/admin/teachers', { email: teacherEmail });
    const temp = (await admin.post('/api/admin/users/reset-password', { email: teacherEmail })).json.temporary_password;
    const c = s.client();
    assert.equal((await c.post('/api/auth/login', { email: teacherEmail, password: temp })).json.must_change_password, true);

    for (const [method, path] of [['GET', '/api/roster'], ['GET', '/api/activities'], ['GET', '/api/topics'], ['GET', '/api/students']]) {
      const r = await c.request(method, path);
      assert.equal(r.status, 403, path);
      assert.equal(r.json.error.code, 'password_change_required', path);
    }
    assert.equal((await c.get('/api/me')).status, 200);

    const ch = await c.post('/api/auth/change-password', { current_password: temp, new_password: 'A-brand-new-pass-9' });
    assert.equal(ch.status, 204);
    assert.equal((await c.get('/api/roster')).status, 200, '更改密碼後放行');
    assert.equal((await c.get('/api/me')).json.must_change_password, false);
  });

  test('臨時密碼：12 字元、不含易混淆字元，且只在建立當下回應一次', async () => {
    const admin = await s.admin();
    const email = `${uniq('tp')}@e2e.test`;
    const r = await admin.post('/api/admin/users/reset-password', { email });
    assert.equal(r.status, 200);
    assert.match(r.json.temporary_password, /^[ABCDEFGHJKMNPQRSTUVWXYZ23456789a]{12}$/);
    assert.equal(r.json.email, email);
    // 之後沒有任何查詢臨時密碼的方式
    assert.ok(!(await admin.get('/api/students')).text.includes(r.json.temporary_password));
    assert.ok(!s.logs().includes(r.json.temporary_password), '臨時密碼不得出現在日誌');
  });

  test('新密碼規則：長度 8–128、不同於目前密碼、需驗證目前密碼', async () => {
    const st = await s.student();
    const cur = st.password;
    const bad = async (body) => (await st.post('/api/auth/change-password', body));

    let r = await bad({ current_password: cur, new_password: 'short7!' });
    assert.equal(r.status, 400);
    assert.equal(r.json.error.code, 'invalid_new_password');
    r = await bad({ current_password: cur, new_password: 'x'.repeat(129) });
    assert.equal(r.json.error.code, 'invalid_new_password');
    r = await bad({ current_password: cur, new_password: cur });
    assert.equal(r.json.error.code, 'password_unchanged');
    r = await bad({ current_password: 'not-my-password', new_password: 'Another-pass-1' });
    assert.equal(r.status, 400);
    assert.equal(r.json.error.code, 'invalid_current_password');

    // 剛好 8 與 128 字元都可以
    r = await bad({ current_password: cur, new_password: '12345678' });
    assert.equal(r.status, 204);
    r = await bad({ current_password: '12345678', new_password: 'y'.repeat(128) });
    assert.equal(r.status, 204);
  });

  test('更改密碼後，該使用者「其他」session 失效，目前這個保留', async () => {
    const st = await s.student();
    const other = s.client();
    assert.equal((await other.post('/api/auth/login', { email: st.email, password: st.password })).status, 200);
    assert.equal((await other.get('/api/me')).status, 200);

    const r = await st.post('/api/auth/change-password', { current_password: st.password, new_password: 'Changed-pass-77' });
    assert.equal(r.status, 204);
    assert.equal((await st.get('/api/me')).status, 200, '目前 session 保留');
    assert.equal((await other.get('/api/me')).status, 401, '其他 session 失效');
    assert.equal((await s.client().post('/api/auth/login', { email: st.email, password: 'Changed-pass-77' })).status, 200);
  });

  test('管理者重設密碼：該使用者所有 session 立即失效，並產生新的臨時密碼', async () => {
    const st = await s.student();
    const admin = await s.admin();
    assert.equal((await st.get('/api/me')).status, 200);

    const r = await admin.post('/api/admin/users/reset-password', { email: st.email });
    assert.equal(r.status, 200);
    assert.equal((await st.get('/api/me')).status, 401);
    assert.equal((await s.client().post('/api/auth/login', { email: st.email, password: st.password })).status, 401, '舊密碼失效');
    const again = s.client();
    const login = await again.post('/api/auth/login', { email: st.email, password: r.json.temporary_password });
    assert.deepEqual(login.json, { must_change_password: true });
  });

  test('重設密碼：只有管理者；信箱格式錯誤回 400；沒有帳號就建立', async () => {
    const t = await s.teacher();
    const st = await s.student();
    assert.equal((await t.post('/api/admin/users/reset-password', { email: st.email })).status, 403, '教師不行');
    assert.equal((await st.post('/api/admin/users/reset-password', { email: st.email })).status, 403, '學生不行');
    assert.equal((await s.client().post('/api/admin/users/reset-password', { email: st.email })).status, 401, '未登入');
    const admin = await s.admin();
    assert.equal((await admin.post('/api/admin/users/reset-password', { email: 'not-an-email' })).json.error.code, 'invalid_email');
    const fresh = `${uniq('fresh')}@e2e.test`;
    assert.equal(s.sql(`SELECT count(*) FROM users WHERE email = '${fresh}'`), '0');
    assert.equal((await admin.post('/api/admin/users/reset-password', { email: fresh })).status, 200);
    assert.equal(s.sql(`SELECT count(*) FROM users WHERE email = '${fresh}'`), '1');
  });
});

describe('S-08.2 §1 登入失敗限流', () => {
  test('連續失敗 5 次鎖定 15 分鐘，期間正確密碼也被拒絕（429 too_many_attempts）', async () => {
    const st = await s.student();
    const c = s.client();
    for (let i = 0; i < 5; i++) {
      const r = await c.post('/api/auth/login', { email: st.email, password: 'wrong-password' });
      assert.equal(r.status, 401, `第 ${i + 1} 次`);
    }
    const locked = await c.post('/api/auth/login', { email: st.email, password: st.password });
    assert.equal(locked.status, 429);
    assert.equal(locked.json.error.code, 'too_many_attempts');
    assert.ok(!c.sid);

    const secs = s.sql(`SELECT round(extract(epoch FROM locked_until - now())) FROM login_failures WHERE email = '${st.email}'`);
    assert.ok(Number(secs) > 14 * 60 && Number(secs) <= 15 * 60, `鎖定約 15 分鐘，實際 ${secs} 秒`);

    // 鎖定到期後恢復
    s.sql(`UPDATE login_failures SET locked_until = now() - interval '1 second' WHERE email = '${st.email}'`);
    assert.equal((await c.post('/api/auth/login', { email: st.email, password: st.password })).status, 200);
  });

  test('不存在的信箱同樣鎖定（避免洩漏帳號是否存在）', async () => {
    const email = `${uniq('ghost')}@e2e.test`;
    const c = s.client();
    for (let i = 0; i < 5; i++) assert.equal((await c.post('/api/auth/login', { email, password: 'wrong-password' })).status, 401);
    assert.equal((await c.post('/api/auth/login', { email, password: 'wrong-password' })).status, 429);
  });

  test('成功登入會歸零計數；鎖定只影響該信箱', async () => {
    const a = await s.student();
    const b = await s.student();
    const c = s.client();
    for (let i = 0; i < 4; i++) await c.post('/api/auth/login', { email: a.email, password: 'wrong-password' });
    assert.equal((await c.post('/api/auth/login', { email: a.email, password: a.password })).status, 200);
    for (let i = 0; i < 4; i++) await c.post('/api/auth/login', { email: a.email, password: 'wrong-password' });
    assert.equal((await c.post('/api/auth/login', { email: a.email, password: a.password })).status, 200, '計數已歸零，仍未鎖定');

    for (let i = 0; i < 5; i++) await c.post('/api/auth/login', { email: a.email, password: 'wrong-password' });
    assert.equal((await c.post('/api/auth/login', { email: a.email, password: a.password })).status, 429);
    assert.equal((await s.client().post('/api/auth/login', { email: b.email, password: b.password })).status, 200, '其他信箱不受影響');
  });

  test('管理者重設密碼會清除登入鎖定', async () => {
    const st = await s.student();
    const c = s.client();
    for (let i = 0; i < 5; i++) await c.post('/api/auth/login', { email: st.email, password: 'wrong-password' });
    assert.equal((await c.post('/api/auth/login', { email: st.email, password: st.password })).status, 429);
    const reset = await (await s.admin()).post('/api/admin/users/reset-password', { email: st.email });
    assert.equal((await c.post('/api/auth/login', { email: st.email, password: reset.json.temporary_password })).status, 200);
  });
});

describe('S-08.2 §4 Session', () => {
  test('未登入呼叫受保護 API → 401 unauthorized，附 request ID', async () => {
    const r = await s.client().get('/api/me');
    assert.equal(r.status, 401);
    assert.equal(r.json.error.code, 'unauthorized');
    assert.equal(r.json.error.request_id, r.headers.get('x-request-id'));
  });

  test('每次登入都建立新 session（防 session fixation）', async () => {
    const st = await s.student();
    const c = s.client();
    await c.post('/api/auth/login', { email: st.email, password: st.password });
    const first = c.sid;
    await c.post('/api/auth/login', { email: st.email, password: st.password });
    assert.notEqual(c.sid, first);
    // 即使帶著舊 cookie 登入，也不沿用舊 token
    const fixed = s.client();
    fixed.cookies.set('sid', 'attacker-chosen-token');
    await fixed.post('/api/auth/login', { email: st.email, password: st.password });
    assert.notEqual(fixed.sid, 'attacker-chosen-token');
  });

  test('登出：刪除 session 紀錄並清除 cookie', async () => {
    const st = await s.student();
    const old = st.sid;
    const r = await st.post('/api/auth/logout');
    assert.equal(r.status, 204);
    assert.match(r.setCookie.find((l) => l.startsWith('sid=')), /Max-Age=0/i);
    assert.equal(s.sql(`SELECT count(*) FROM sessions WHERE encode(token_hash,'hex') = '${sha256hex(old)}'`), '0');
    const replay = s.client();
    replay.cookies.set('sid', old);
    assert.equal((await replay.get('/api/me')).status, 401);
  });

  test('閒置超過 7 天 → 401；未滿 7 天仍有效且更新 last_seen_at', async () => {
    const st = await s.student();
    const where = `encode(token_hash,'hex') = '${sha256hex(st.sid)}'`;
    s.sql(`UPDATE sessions SET last_seen_at = now() - interval '6 days' WHERE ${where}`);
    assert.equal((await st.get('/api/me')).status, 200);
    const age = Number(s.sql(`SELECT extract(epoch FROM now() - last_seen_at) FROM sessions WHERE ${where}`));
    assert.ok(age < 60, '使用後 last_seen_at 應更新為現在');

    s.sql(`UPDATE sessions SET last_seen_at = now() - interval '7 days 1 minute' WHERE ${where}`);
    assert.equal((await st.get('/api/me')).status, 401);
  });

  test('即使持續使用，建立超過 30 天的 session 失效（絕對上限）', async () => {
    const st = await s.student();
    s.sql(`UPDATE sessions SET created_at = now() - interval '30 days 1 minute' WHERE encode(token_hash,'hex') = '${sha256hex(st.sid)}'`);
    assert.equal((await st.get('/api/me')).status, 401);
  });

  test('偽造或亂填的 cookie → 401', async () => {
    const c = s.client();
    c.cookies.set('sid', 'x'.repeat(43));
    assert.equal((await c.get('/api/me')).status, 401);
  });
});

describe('S-08.2 §4 CSRF：改變狀態的請求 Origin 必須等於本站', () => {
  test('Origin 不符或缺少的 POST/PATCH/DELETE → 403 csrf，且沒有任何效果', async () => {
    const st = await s.student();
    for (const origin of ['https://evil.example', null, 'null']) {
      const r = await st.post('/api/auth/change-password', { current_password: st.password, new_password: 'Hijacked-pass-1' }, { origin });
      assert.equal(r.status, 403, `origin=${origin}`);
      assert.equal(r.json.error.code, 'csrf');
    }
    assert.equal((await st.request('DELETE', '/api/conversations/00000000-0000-0000-0000-000000000000', { origin: 'https://evil.example' })).status, 403);
    assert.equal((await s.client().post('/api/auth/login', { email: st.email, password: st.password })).status, 200, '密碼未被更動');
  });

  test('登入端點也受保護；GET 不檢查 Origin', async () => {
    const st = await s.student();
    const evil = await s.client().post('/api/auth/login', { email: st.email, password: st.password }, { origin: 'https://evil.example' });
    assert.equal(evil.status, 403);
    assert.equal((await st.get('/api/me', { origin: 'https://evil.example' })).status, 200);
  });
});

describe('S-08.2 §2 Google 登入入口（OIDC Authorization Code + PKCE）', () => {
  test('/api/auth/google/login 導向 Google，帶 state、nonce、PKCE S256，並暫存到資料庫', async () => {
    const r = await s.client().get('/api/auth/google/login');
    assert.ok([302, 303, 307].includes(r.status));
    const loc = new URL(r.headers.get('location'));
    assert.equal(loc.origin, 'https://accounts.google.com');
    const q = loc.searchParams;
    assert.equal(q.get('response_type'), 'code');
    assert.equal(q.get('client_id'), 'e2e-client-id');
    assert.equal(q.get('redirect_uri'), `${s.base}/api/auth/google/callback`);
    assert.deepEqual(q.get('scope').split(' ').sort(), ['email', 'openid', 'profile']);
    assert.equal(q.get('code_challenge_method'), 'S256');
    assert.ok(q.get('code_challenge'));
    assert.ok(q.get('state') && q.get('nonce'));
    assert.equal(s.sql(`SELECT count(*) FROM oauth_login_states WHERE state = '${q.get('state')}'`), '1');
    assert.ok(!s.sql(`SELECT pkce_verifier FROM oauth_login_states WHERE state = '${q.get('state')}'`).includes(q.get('code_challenge')), '只把 challenge 給 Google，verifier 留在後端');
  });

  test('callback 的 state 不存在 → 導回登入頁並附錯誤，不建立 session', async () => {
    const c = s.client();
    const r = await c.get('/api/auth/google/callback?code=abc&state=does-not-exist');
    assert.ok([302, 303, 307].includes(r.status));
    assert.equal(new URL(r.headers.get('location'), s.base).pathname, '/login');
    assert.equal(new URL(r.headers.get('location'), s.base).searchParams.get('error'), 'invalid_state');
    assert.ok(!c.sid);
  });

  async function freshState() {
    const r = await s.client().get('/api/auth/google/login');
    return new URL(r.headers.get('location')).searchParams.get('state');
  }

  test('使用者取消授權（access_denied）→ 導回登入頁；state 單次使用', async () => {
    const state = await freshState();
    const c = s.client();
    const r = await c.get(`/api/auth/google/callback?error=access_denied&state=${state}`);
    assert.equal(new URL(r.headers.get('location'), s.base).searchParams.get('error'), 'access_denied');
    assert.ok(!c.sid);
    assert.equal(s.sql(`SELECT count(*) FROM oauth_login_states WHERE state = '${state}'`), '0', 'state 已被取出刪除');
    const replay = await c.get(`/api/auth/google/callback?code=abc&state=${state}`);
    assert.equal(new URL(replay.headers.get('location'), s.base).searchParams.get('error'), 'invalid_state');
  });

  test('state 超過 10 分鐘 → 拒絕', async () => {
    const state = await freshState();
    s.sql(`UPDATE oauth_login_states SET created_at = now() - interval '11 minutes' WHERE state = '${state}'`);
    const r = await s.client().get(`/api/auth/google/callback?code=abc&state=${state}`);
    assert.equal(new URL(r.headers.get('location'), s.base).searchParams.get('error'), 'invalid_state');
  });

  test('缺少 code → 拒絕，不建立 session', async () => {
    const state = await freshState();
    const c = s.client();
    const r = await c.get(`/api/auth/google/callback?state=${state}`);
    assert.equal(new URL(r.headers.get('location'), s.base).searchParams.get('error'), 'invalid_state');
    assert.ok(!c.sid);
  });
});
