// 題目／活動（教師建立討論活動）與資料可見範圍：S-02.1、S-02.2、S-02.5、AGENTS「資安與個資底線」。
import { describe, test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { startStack, uniq } from '../lib/stack.mjs';

let s;
before(async () => { s = await startStack(); });
after(async () => { await s?.stop(); });

describe('題目與活動', () => {
  test('內建題目「電車難題」可供學生選擇', async () => {
    const st = await s.student();
    const avail = await st.get('/api/available');
    assert.equal(avail.status, 200);
    assert.ok(avail.json.topics.some((t) => t.title === '電車難題'));
  });

  test('題目：管理者與教師可建立、修改；標題不可為空；停用後學生看不到也不能開新對話，既有對話不受影響', async () => {
    const admin = await s.admin();
    const t = await s.teacher();
    const st = await s.student();
    assert.equal((await admin.post('/api/admin/topics', { title: '  ' })).json.error.code, 'invalid_title');
    const topic = (await t.post('/api/topics', { title: '說謊的道德性', description: '善意的謊言可以接受嗎？', category: '倫理學' })).json;
    assert.equal(topic.is_active, true);
    assert.equal((await st.post('/api/topics', { title: 'x' })).status, 403);

    const conv = await s.startConversation(st, { topic_id: topic.id });
    assert.equal(conv.title, '說謊的道德性');

    const off = await admin.patch(`/api/admin/topics/${topic.id}`, { is_active: false, title: '改名了' });
    assert.equal(off.json.is_active, false);
    assert.ok(!(await st.get('/api/available')).json.topics.some((x) => x.id === topic.id));
    const blocked = await st.post('/api/conversations', { topic_id: topic.id });
    assert.equal(blocked.status, 400);
    assert.equal(blocked.json.error.code, 'invalid_source');
    const existing = await st.get(`/api/conversations/${conv.id}`);
    assert.equal(existing.json.title, '說謊的道德性', '既有對話保留開始時複製的題目內容');
    assert.equal((await t.patch(`/api/topics/${topic.id}`, { title: '' })).json.error.code, 'invalid_title');
  });

  test('活動：草稿學生看不到；發布後可選；開始對話時複製內容；關閉後不能再開始', async () => {
    const t = await s.teacher();
    const st = await s.student();
    assert.equal((await t.post('/api/activities', { title: '' })).json.error.code, 'invalid_title');
    assert.equal((await t.post('/api/activities', { title: 'x', topic_id: '00000000-0000-4000-8000-0000000000ff' })).json.error.code, 'invalid_topic');

    const act = (await t.post('/api/activities', { title: '第一週討論', description: '請談談你的看法' })).json;
    assert.equal(act.status, 'draft');
    assert.ok(!(await st.get('/api/available')).json.activities.some((a) => a.id === act.id));
    assert.equal((await st.post('/api/conversations', { activity_id: act.id })).status, 400, '草稿不可開始');

    const pub = await t.post(`/api/activities/${act.id}/publish`);
    assert.equal(pub.json.status, 'published');
    assert.ok((await st.get('/api/available')).json.activities.some((a) => a.id === act.id));
    const conv = await s.startConversation(st, { activity_id: act.id });
    assert.deepEqual([conv.title, conv.description, conv.activity_id], ['第一週討論', '請談談你的看法', act.id]);

    await t.patch(`/api/activities/${act.id}`, { title: '改名後' });
    assert.equal((await st.get(`/api/conversations/${conv.id}`)).json.title, '第一週討論', '來源異動不影響既有對話');

    assert.equal((await t.post(`/api/activities/${act.id}/close`)).json.status, 'closed');
    assert.ok(!(await st.get('/api/available')).json.activities.some((a) => a.id === act.id));
    assert.equal((await st.post('/api/conversations', { activity_id: act.id })).status, 400);
    assert.equal((await t.get(`/api/activities/${'00000000-0000-4000-8000-0000000000ee'}`)).status, 404);
    assert.equal((await st.post('/api/activities', { title: 'x' })).status, 403);
  });

  test('開始對話必須指定活動或題目', async () => {
    const st = await s.student();
    const r = await st.post('/api/conversations', {});
    assert.equal(r.status, 400);
    assert.equal(r.json.error.code, 'invalid_source');
  });
});

describe('S-02.1／S-02.2 教師看不到原文；他人不能存取對話', () => {
  const MARKER = 'MARKER-RAW-TEXT-不可外流';

  test('對話只有擁有者能讀寫：其他學生一律 404（不洩漏是否存在）；教師、管理者、未登入被擋', async () => {
    const owner = await s.student();
    const other = await s.student();
    const t = await s.teacher();
    const admin = await s.admin();
    const conv = await s.startConversation(owner);
    await s.turn(owner, conv.id, MARKER);

    const id = conv.id;
    assert.equal((await other.get('/api/conversations')).json.length, 0, '清單只含自己的');
    for (const [m, p, body] of [
      ['GET', `/api/conversations/${id}`], ['DELETE', `/api/conversations/${id}`],
      ['POST', `/api/conversations/${id}/messages`, { content: 'hi' }], ['POST', `/api/conversations/${id}/end`],
      ['GET', `/api/conversations/${id}/summary`], ['POST', `/api/conversations/${id}/summary/retry`],
    ]) {
      const r = await other.request(m, p, body ? { json: body } : {});
      assert.equal(r.status, 404, `${m} ${p}`);
      assert.ok(!r.text.includes(MARKER));
    }
    assert.equal((await other.sse(`/api/conversations/${id}/stream`)).status, 404);

    for (const c of [t, admin]) {
      const r = await c.get(`/api/conversations/${id}`);
      assert.equal(r.status, 403, '教師與管理者看不到對話原文');
      assert.ok(!r.text.includes(MARKER));
      assert.equal((await c.get('/api/conversations')).status, 403);
    }
    assert.equal((await s.client().get(`/api/conversations/${id}`)).status, 401);
    assert.equal((await owner.get(`/api/conversations/${id}`)).status, 200);
  });

  test('教師端只有 AI 總結，任何教師 API 都不含對話原文', async () => {
    const st = await s.student();
    const t = await s.teacher();
    const { conv } = await s.completeConversation(st, { turns: 2 });
    await st.post(`/api/conversations/${conv.id}/messages`, { content: MARKER }).catch(() => {});
    const dump = [
      (await t.get('/api/teacher/summaries')).text, (await t.get('/api/dashboard/class')).text,
      (await t.get('/api/students')).text, (await t.get('/api/roster')).text,
    ].join('\n');
    assert.ok(!dump.includes(MARKER));
    assert.ok(!dump.includes('我的看法'), '學生訊息內容不在教師 API 中');
    const row = (await t.get('/api/teacher/summaries')).json.find((r) => r.conversation_id === conv.id);
    assert.deepEqual(Object.keys(row).sort(), ['claim', 'conversation_id', 'ended_at', 'framework', 'framework_scores', 'reasons', 'stance', 'student_email', 'student_id', 'student_name', 'title']);
  });

  test('管理者看到的敏感欄位一律以固定字串 message 代替（S-02.2）', async () => {
    const st = await s.student();
    const admin = await s.admin();
    const { conv } = await s.completeConversation(st);
    const row = (await admin.get('/api/teacher/summaries')).json.find((r) => r.conversation_id === conv.id);
    assert.equal(row.stance, 'message');
    assert.equal(row.reasons, 'message');
    assert.equal(row.claim, 'message');
    assert.equal(row.framework, 'message');
    assert.equal(row.framework_scores, null);
    assert.equal(row.student_email, st.email, '管理者看得到姓名與電子郵件（管理帳號需要）');
    const text = (await admin.get('/api/teacher/summaries')).text;
    assert.ok(!text.includes('支持拉桿') && !text.includes('五條人命'));
  });

  test('學生刪除自己的單場對話：訊息、總結一併刪除，教師也不再看到；別人的刪不掉', async () => {
    const st = await s.student();
    const other = await s.student();
    const t = await s.teacher();
    const { conv } = await s.completeConversation(st);
    assert.equal((await other.del(`/api/conversations/${conv.id}`)).status, 404);
    assert.ok((await t.get('/api/teacher/summaries')).json.some((r) => r.conversation_id === conv.id));

    assert.equal((await st.del(`/api/conversations/${conv.id}`)).status, 204);
    assert.equal((await st.get(`/api/conversations/${conv.id}`)).status, 404);
    assert.equal(s.sql(`SELECT count(*) FROM messages WHERE conversation_id = '${conv.id}'`), '0');
    assert.equal(s.sql(`SELECT count(*) FROM summaries WHERE conversation_id = '${conv.id}'`), '0');
    assert.ok(!(await t.get('/api/teacher/summaries')).json.some((r) => r.conversation_id === conv.id));
  });

  test('日誌（tracing）不含聊天原文、密碼、token、電子郵件、AI 金鑰', async () => {
    const st = await s.student();
    const conv = await s.startConversation(st);
    await s.turn(st, conv.id, MARKER);
    await st.post('/api/auth/login', { email: st.email, password: 'wrong-password-xyz' });
    const logs = s.logs();
    assert.ok(logs.length > 0, '後端確實有輸出日誌');
    for (const secret of [MARKER, st.password, 'wrong-password-xyz', st.sid, s.fake.apiKey, '@e2e.test', 'Init-Admin-Pass-1']) {
      assert.ok(!logs.includes(secret), `日誌不應包含：${secret}`);
    }
    assert.ok(!uniq().includes(' '));
  });
});
