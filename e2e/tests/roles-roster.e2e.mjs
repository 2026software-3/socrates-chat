// S-01.3 角色核發與停用、S-01.4 修課名單、S-02.5 帳號刪除、S-08.2 §3 §5 角色判定與授權。
import { describe, test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { startStack, uniq, ADMIN_EMAIL } from '../lib/stack.mjs';

let s;
before(async () => { s = await startStack(); });
after(async () => { await s?.stop(); });

const email = (p) => `${uniq(p)}@e2e.test`;

describe('S-01.4 修課名單匯入', () => {
  test('去空白、轉小寫、去重；有無 email 標題列皆可；回報新增／已存在／格式錯誤與錯誤行號', async () => {
    const t = await s.teacher();
    const a = email('a'); const b = email('b');
    const text = ['email', `  ${a.toUpperCase()} `, `${b},王小明,extra`, '', 'bad-line', a, 'x@@y'].join('\n');
    const r = await t.post('/api/roster/import', { text });
    assert.equal(r.status, 200);
    assert.equal(r.json.added, 2);
    assert.equal(r.json.existing, 0);
    assert.deepEqual(r.json.invalid, [{ line: 5, value: 'bad-line' }, { line: 7, value: 'x@@y' }]);

    const list = await t.get('/api/roster');
    assert.ok(list.json.emails.includes(a) && list.json.emails.includes(b), '以小寫保存');
    assert.ok(!list.json.emails.some((e) => e !== e.toLowerCase()));

    const again = await t.post('/api/roster/import', { text: `${a}\n${b}` });
    assert.equal(again.json.added, 0);
    assert.equal(again.json.existing, 2);
  });

  test('沒有標題列的純信箱清單也可匯入', async () => {
    const t = await s.teacher();
    const a = email('n1'); const b = email('n2');
    const r = await t.post('/api/roster/import', { text: `${a}\r\n${b}\r\n` });
    assert.equal(r.json.added, 2);
    assert.deepEqual(r.json.invalid, []);
  });

  test('匯入只會新增，不會移除名單上原有但這次沒有的人', async () => {
    const t = await s.teacher();
    const first = email('keep'); const second = email('add');
    await t.post('/api/roster/import', { text: first });
    await t.post('/api/roster/import', { text: second });
    const emails = (await t.get('/api/roster')).json.emails;
    assert.ok(emails.includes(first) && emails.includes(second));
  });

  test('教師匯入只新增名單：不建立帳號、不回臨時密碼', async () => {
    const t = await s.teacher();
    const a = email('tnoacct');
    const r = await t.post('/api/roster/import', { text: a });
    assert.equal(r.json.added, 1);
    assert.deepEqual(r.json.credentials, []);
    assert.equal(s.sql(`SELECT count(*) FROM users WHERE email = '${a}'`), '0');
  });

  test('管理者匯入：只有「新加入、沒有帳號、非教師／管理者」的信箱取得臨時密碼，且不會重發', async () => {
    const admin = await s.admin();
    const fresh = email('fresh');
    const hasAccount = email('hasacct');
    const teacher = email('staff');
    s.sql(`INSERT INTO users (email, google_sub) VALUES ('${hasAccount}', 'sub-${uniq()}')`);
    await admin.post('/api/admin/teachers', { email: teacher });

    const r = await admin.post('/api/roster/import', { text: [fresh, hasAccount, teacher, ADMIN_EMAIL].join('\n') });
    assert.equal(r.json.added, 4);
    assert.deepEqual(r.json.credentials.map((c) => c.email), [fresh]);
    assert.match(r.json.credentials[0].temporary_password, /^[A-Za-z0-9]{12}$/);

    const login = await s.client().post('/api/auth/login', { email: fresh, password: r.json.credentials[0].temporary_password });
    assert.deepEqual(login.json, { must_change_password: true });
    assert.equal(s.sql(`SELECT count(*) FROM users WHERE email = '${teacher}'`), '0', '教師信箱不建立帳號');

    const again = await admin.post('/api/roster/import', { text: fresh });
    assert.equal(again.json.added, 0);
    assert.deepEqual(again.json.credentials, [], '再次匯入不會重發');
    // 管理者信箱被加進名單會同時成為學生；清掉以免影響其他測試
    await admin.del(`/api/roster/${encodeURIComponent(ADMIN_EMAIL)}`);
  });

  test('先由教師匯入的信箱，之後管理者匯入不會補發臨時密碼（僅「新加入」才發）', async () => {
    const t = await s.teacher();
    const admin = await s.admin();
    const a = email('first-by-teacher');
    await t.post('/api/roster/import', { text: a });
    const r = await admin.post('/api/roster/import', { text: a });
    assert.equal(r.json.added, 0);
    assert.deepEqual(r.json.credentials, []);
  });

  test('名單與學生清單只有教師／管理者可看；學生與未登入被擋', async () => {
    const st = await s.student();
    assert.equal((await st.get('/api/roster')).status, 403);
    assert.equal((await st.post('/api/roster/import', { text: email('x') })).status, 403);
    assert.equal((await st.get('/api/students')).status, 403);
    assert.equal((await s.client().get('/api/roster')).status, 401);
    assert.equal((await s.client().post('/api/roster/import', { text: email('x') })).status, 401);
    assert.equal((await (await s.admin()).get('/api/roster')).status, 200, '管理者不必在教師清單內也能管理名單');
  });
});

describe('S-01.4 移出名單與重新加回', () => {
  test('移出名單：立即失去學生權限（not_enrolled），資料保留、教師仍看得到總結；重新加回即恢復', async () => {
    const t = await s.teacher();
    const st = await s.student();
    const { conv } = await s.completeConversation(st);

    const before = await t.get('/api/teacher/summaries');
    assert.ok(before.json.some((r) => r.conversation_id === conv.id));

    const rm = await t.del(`/api/roster/${encodeURIComponent(st.email)}`);
    assert.equal(rm.status, 204);
    const denied = await st.get('/api/conversations');
    assert.equal(denied.status, 403, '下一次請求就失去權限，不必重新登入');
    assert.equal(denied.json.error.code, 'not_enrolled');
    assert.equal((await st.get('/api/me')).json.roles.student, false);

    // 資料全部保留
    assert.equal(s.sql(`SELECT count(*) FROM conversations WHERE id = '${conv.id}'`), '1');
    assert.ok((await t.get('/api/teacher/summaries')).json.some((r) => r.conversation_id === conv.id), '教師仍看得到過去的總結');

    // 重新加回
    await t.post('/api/roster/import', { text: st.email });
    const back = await st.get(`/api/conversations/${conv.id}`);
    assert.equal(back.status, 200);
    assert.equal(back.json.id, conv.id);
  });

  test('移除不在名單上的信箱 → 404；信箱不分大小寫', async () => {
    const t = await s.teacher();
    assert.equal((await t.del(`/api/roster/${encodeURIComponent(email('nobody'))}`)).status, 404);
    const a = email('MixedCase');
    await t.post('/api/roster/import', { text: a });
    assert.equal((await t.del(`/api/roster/${encodeURIComponent(a.toUpperCase())}`)).status, 204);
  });
});

describe('S-08.2 §3 §5 角色判定', () => {
  test('名單外、非教師、非管理者的帳號呼叫學生 API → 403 not_enrolled', async () => {
    const admin = await s.admin();
    const e = email('outsider');
    const temp = (await admin.post('/api/admin/users/reset-password', { email: e })).json.temporary_password;
    const c = await s.signIn(e, temp);
    for (const p of ['/api/available', '/api/conversations', '/api/dashboard/me']) {
      const r = await c.get(p);
      assert.equal(r.status, 403, p);
      assert.equal(r.json.error.code, 'not_enrolled', p);
    }
    const me = await c.get('/api/me');
    assert.deepEqual(me.json.roles, { admin: false, teacher: false, student: false });
  });

  test('教師與管理者不參與討論：呼叫學生 API → 403 forbidden（且不需要在修課名單內）', async () => {
    const t = await s.teacher();
    const admin = await s.admin();
    for (const c of [t, admin]) {
      const r = await c.get('/api/conversations');
      assert.equal(r.status, 403);
      assert.equal(r.json.error.code, 'forbidden');
    }
  });

  test('學生不能呼叫教師／管理者 API；教師不能呼叫管理者 API', async () => {
    const st = await s.student();
    const t = await s.teacher();
    for (const [m, p] of [['GET', '/api/students'], ['GET', '/api/activities'], ['GET', '/api/teacher/summaries'], ['GET', '/api/dashboard/class']]) {
      assert.equal((await st.request(m, p)).status, 403, `student ${p}`);
    }
    for (const [m, p] of [['GET', '/api/admin/teachers'], ['GET', '/api/admin/topics']]) {
      assert.equal((await st.request(m, p)).status, 403, `student ${p}`);
      assert.equal((await t.request(m, p)).status, 403, `teacher ${p}`);
    }
  });

  test('角色每次請求從資料庫載入：移出名單、新增／移除教師立即生效', async () => {
    const admin = await s.admin();
    const e = email('rolechange');
    const temp = (await admin.post('/api/admin/users/reset-password', { email: e })).json.temporary_password;
    const c = await s.signIn(e, temp);
    assert.equal((await c.get('/api/me')).json.roles.teacher, false);
    assert.equal((await c.get('/api/roster')).status, 403);

    assert.equal((await admin.post('/api/admin/teachers', { email: e.toUpperCase() })).status, 204);
    assert.equal((await c.get('/api/me')).json.roles.teacher, true, '已登入的帳號立即成為教師');
    assert.equal((await c.get('/api/roster')).status, 200);

    assert.equal((await admin.del(`/api/admin/teachers/${encodeURIComponent(e)}`)).status, 204);
    assert.equal((await c.get('/api/roster')).status, 403, '移除教師身分立即生效');
  });
});

describe('S-01.3 教師清單管理（僅管理者）', () => {
  test('新增（冪等）、列出、移除；格式錯誤 400、移除不存在 404', async () => {
    const admin = await s.admin();
    const e = email('teach');
    assert.equal((await admin.post('/api/admin/teachers', { email: e })).status, 204);
    assert.equal((await admin.post('/api/admin/teachers', { email: e })).status, 204, '重複新增不報錯');
    assert.equal((await admin.get('/api/admin/teachers')).json.emails.filter((x) => x === e).length, 1);
    assert.equal((await admin.post('/api/admin/teachers', { email: 'nope' })).json.error.code, 'invalid_email');
    assert.equal((await admin.del(`/api/admin/teachers/${encodeURIComponent(e)}`)).status, 204);
    assert.equal((await admin.del(`/api/admin/teachers/${encodeURIComponent(e)}`)).status, 404);
  });

  test('教師、學生、未登入都不能管理教師清單', async () => {
    const t = await s.teacher();
    const st = await s.student();
    const target = email('x');
    for (const c of [t, st]) {
      assert.equal((await c.post('/api/admin/teachers', { email: target })).status, 403);
      assert.equal((await c.get('/api/admin/teachers')).status, 403);
      assert.equal((await c.del(`/api/admin/teachers/${encodeURIComponent(target)}`)).status, 403);
    }
    assert.equal((await s.client().post('/api/admin/teachers', { email: target })).status, 401);
  });
});

describe('S-01.3 帳號停用與復原', () => {
  test('停用：立即清除所有 session、登入看到 account_disabled、資料保留；復原後照舊；兩次操作都寫稽核日誌', async () => {
    const admin = await s.admin();
    const st = await s.student();
    const { conv } = await s.completeConversation(st);
    const second = s.client();
    await second.post('/api/auth/login', { email: st.email, password: st.password });

    const off = await admin.post(`/api/admin/users/${encodeURIComponent(st.email)}/disable`, { reason: '  測試停用  ' });
    assert.equal(off.status, 204);
    assert.equal((await st.get('/api/me')).status, 401, '既有 session 立即失效');
    assert.equal((await second.get('/api/me')).status, 401);
    const login = await s.client().post('/api/auth/login', { email: st.email, password: st.password });
    assert.equal(login.status, 403);
    assert.equal(login.json.error.code, 'account_disabled');
    assert.equal(s.sql(`SELECT count(*) FROM conversations WHERE id = '${conv.id}'`), '1', '資料全部保留');
    assert.equal(s.sql(`SELECT count(*) FROM sessions s JOIN users u ON u.id = s.user_id WHERE u.email = '${st.email}'`), '0');

    const on = await admin.post(`/api/admin/users/${encodeURIComponent(st.email)}/enable`, { reason: '誤判' });
    assert.equal(on.status, 204);
    const back = await s.signIn(st.email, st.password);
    assert.equal((await back.get(`/api/conversations/${conv.id}`)).status, 200, '角色與資料照舊');

    const rows = s.sql(`SELECT action || '|' || coalesce(reason,'') || '|' || (SELECT email FROM users WHERE id = actor_id) FROM audit_logs WHERE target_email = '${st.email}' ORDER BY created_at`).split('\n');
    assert.deepEqual(rows, [`account_disabled|測試停用|${ADMIN_EMAIL}`, `account_enabled|誤判|${ADMIN_EMAIL}`]);
  });

  test('重複停用／復原是冪等的，不會重複寫稽核日誌', async () => {
    const admin = await s.admin();
    const st = await s.student();
    const url = (a) => `/api/admin/users/${encodeURIComponent(st.email)}/${a}`;
    assert.equal((await admin.post(url('enable'))).status, 204);
    assert.equal((await admin.post(url('disable'))).status, 204);
    assert.equal((await admin.post(url('disable'))).status, 204);
    assert.equal((await admin.post(url('enable'))).status, 204);
    assert.equal((await admin.post(url('enable'))).status, 204);
    assert.equal(s.sql(`SELECT count(*) FROM audit_logs WHERE target_email = '${st.email}'`), '2');
  });

  test('管理者不能停用自己；停用不存在的帳號 404；非管理者 403', async () => {
    const admin = await s.admin();
    const self = await admin.post(`/api/admin/users/${encodeURIComponent(ADMIN_EMAIL)}/disable`);
    assert.equal(self.status, 409);
    assert.equal(self.json.error.code, 'cannot_disable_self');
    assert.equal((await admin.get('/api/me')).status, 200);
    assert.equal((await admin.post(`/api/admin/users/${encodeURIComponent(email('ghost'))}/disable`)).status, 404);
    assert.equal((await admin.post(`/api/admin/users/${encodeURIComponent(email('ghost'))}/enable`)).status, 404);

    const t = await s.teacher();
    const st = await s.student();
    assert.equal((await t.post(`/api/admin/users/${encodeURIComponent(st.email)}/disable`)).status, 403);
    assert.equal((await st.post(`/api/admin/users/${encodeURIComponent(st.email)}/disable`)).status, 403);
  });

  test('管理者可以停用另一位管理者（只要仍有其他啟用中的管理者）', async () => {
    const admin = await s.admin();
    const second = await s.signIn('admin2@e2e.test', 'Init-Admin-Pass-1');
    assert.equal((await admin.post('/api/admin/users/admin2%40e2e.test/disable')).status, 204);
    assert.equal((await second.get('/api/me')).status, 401);
    assert.equal((await admin.post('/api/admin/users/admin2%40e2e.test/enable')).status, 204);
  });
});

describe('S-02.5 刪除學生帳號（教師或管理者）', () => {
  test('需再次確認信箱；刪除後名單、帳號、對話與總結一併消失，並寫稽核日誌', async () => {
    const t = await s.teacher();
    const st = await s.student();
    const { conv } = await s.completeConversation(st);

    const wrong = await t.post(`/api/students/${encodeURIComponent(st.email)}/delete`, { confirm_email: email('other') });
    assert.equal(wrong.status, 400);
    assert.equal(wrong.json.error.code, 'confirmation_mismatch');
    assert.equal(s.sql(`SELECT count(*) FROM users WHERE email = '${st.email}'`), '1', '確認不符不會刪除');

    const ok = await t.post(`/api/students/${encodeURIComponent(st.email)}/delete`, { confirm_email: st.email.toUpperCase() });
    assert.equal(ok.status, 204);
    assert.equal(s.sql(`SELECT count(*) FROM users WHERE email = '${st.email}'`), '0');
    assert.equal(s.sql(`SELECT count(*) FROM enrollments WHERE email = '${st.email}'`), '0');
    assert.equal(s.sql(`SELECT count(*) FROM conversations WHERE id = '${conv.id}'`), '0');
    assert.equal(s.sql(`SELECT count(*) FROM summaries WHERE conversation_id = '${conv.id}'`), '0');
    assert.ok(!(await t.get('/api/teacher/summaries')).json.some((r) => r.conversation_id === conv.id));
    assert.equal((await st.get('/api/me')).status, 401);
    assert.equal((await s.client().post('/api/auth/login', { email: st.email, password: st.password })).status, 401);
    assert.equal(s.sql(`SELECT count(*) FROM audit_logs WHERE action = 'student_deleted' AND target_email = '${st.email}'`), '1');
  });

  test('學生不能自行刪除帳號（也不能刪別人）；未登入被擋', async () => {
    const a = await s.student();
    const b = await s.student();
    assert.equal((await a.post(`/api/students/${encodeURIComponent(a.email)}/delete`, { confirm_email: a.email })).status, 403);
    assert.equal((await a.post(`/api/students/${encodeURIComponent(b.email)}/delete`, { confirm_email: b.email })).status, 403);
    assert.equal((await s.client().post(`/api/students/${encodeURIComponent(b.email)}/delete`, { confirm_email: b.email })).status, 401);
    assert.equal(s.sql(`SELECT count(*) FROM users WHERE email IN ('${a.email}','${b.email}')`), '2');
  });

  test('只能刪除學生帳號：教師與管理者回 409 not_a_student；不存在回 404', async () => {
    const admin = await s.admin();
    const t = await s.teacher();
    const r1 = await admin.post(`/api/students/${encodeURIComponent(t.email)}/delete`, { confirm_email: t.email });
    assert.equal(r1.status, 409);
    assert.equal(r1.json.error.code, 'not_a_student');
    assert.equal((await admin.post(`/api/students/${encodeURIComponent(ADMIN_EMAIL)}/delete`, { confirm_email: ADMIN_EMAIL })).status, 409);
    const ghost = email('ghost');
    assert.equal((await admin.post(`/api/students/${encodeURIComponent(ghost)}/delete`, { confirm_email: ghost })).status, 404);
  });

  test('學生清單：顯示是否已有帳號、是否停用、已完成對話數', async () => {
    const t = await s.teacher();
    const admin = await s.admin();
    const st = await s.student();
    await s.completeConversation(st);
    const noAcct = email('noacct');
    await t.post('/api/roster/import', { text: noAcct });
    await admin.post(`/api/admin/users/${encodeURIComponent(st.email)}/disable`);

    const rows = (await t.get('/api/students')).json;
    const mine = rows.find((r) => r.email === st.email);
    assert.deepEqual({ has_account: mine.has_account, disabled: mine.disabled, done: Number(mine.completed_conversations) }, { has_account: true, disabled: true, done: 1 });
    const other = rows.find((r) => r.email === noAcct);
    assert.deepEqual({ has_account: other.has_account, disabled: other.disabled, done: Number(other.completed_conversations) }, { has_account: false, disabled: false, done: 0 });
    await admin.post(`/api/admin/users/${encodeURIComponent(st.email)}/enable`);
  });
});
