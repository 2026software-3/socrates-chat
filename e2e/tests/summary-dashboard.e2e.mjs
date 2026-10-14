// S-03.3 / F-08 AI 總結（不可修改、可重試）、S-04.2 雷達圖維度、論點分布儀表板（教師）與個人儀表板。
import { describe, test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { startStack, until } from '../lib/stack.mjs';
import { DEFAULT_SUMMARY } from '../lib/fake-openai.mjs';

const SCHOOLS = ['utilitarianism', 'deontology', 'virtue', 'contractarianism', 'care', 'existentialism'];
const summaryOf = (framework, scores, claim = '該拉桿') => ({ json: { ...DEFAULT_SUMMARY, framework, claim, scores } });
const scores = (o) => Object.fromEntries(SCHOOLS.map((k) => [k, o[k] ?? 0]));

describe('總結', () => {
  let s;
  before(async () => { s = await startStack(); });
  after(async () => { await s?.stop(); });

  test('結束後產生總結：立場、理由、主張、學派標籤、6 維分數；規則版本記錄在總結上', async () => {
    const st = await s.student();
    const { conv, summary } = await s.completeConversation(st);
    assert.equal(summary.status, 'ready');
    assert.equal(summary.stance, '支持拉桿');
    assert.equal(summary.reasons, '五條人命比一條人命重要');
    assert.equal(summary.claim, '該拉桿');
    assert.equal(summary.framework, 'utilitarianism');
    assert.deepEqual(Object.keys(summary.framework_scores).sort(), [...SCHOOLS].sort());
    assert.ok(summary.completed_at);
    assert.equal(s.sql(`SELECT rules_version FROM summaries WHERE conversation_id = '${conv.id}'`), 'v1/zh-TW');
    const req = s.fake.of('summary').at(-1);
    assert.ok(req.messages[0].content.includes('我的看法 1'), '總結依完整對話產生');
    assert.ok(req.system.includes('不加入你自己的看法'));
  });

  test('總結不可修改：沒有任何修改端點', async () => {
    const st = await s.student();
    const { conv } = await s.completeConversation(st);
    for (const m of ['PUT', 'PATCH', 'POST']) {
      const r = await st.request(m, `/api/conversations/${conv.id}/summary`, { json: { stance: '偷改' } });
      assert.ok([404, 405].includes(r.status), `${m} → ${r.status}`);
    }
    assert.equal((await st.get(`/api/conversations/${conv.id}/summary`)).json.stance, '支持拉桿');
  });

  test('分數超出範圍夾到 0–5；缺任何學派則整組不用；無法辨識的學派標籤不採用', async () => {
    const st = await s.student();
    s.fake.enqueue('summary', { json: { ...DEFAULT_SUMMARY, framework: 'made-up', scores: scores({ utilitarianism: 99, deontology: -3, virtue: 2.6 }) } });
    const a = await s.completeConversation(st);
    assert.equal(a.summary.framework, null);
    assert.deepEqual(a.summary.framework_scores, scores({ utilitarianism: 5, deontology: 0, virtue: 3 }));

    const partial = { ...scores({}) }; delete partial.care;
    s.fake.enqueue('summary', { json: { ...DEFAULT_SUMMARY, scores: partial } });
    const b = await s.completeConversation(st);
    assert.equal(b.summary.framework_scores, null);
    assert.equal(b.summary.stance, '支持拉桿', '分數缺漏不影響總結本身');
  });

  test('AI 結果不完整（缺理由）→ 自動再試 1 次仍失敗則標記 failed；學生可重試；成功後不能再重試', async () => {
    const st = await s.student();
    const conv = await s.startConversation(st);
    await s.turn(st, conv.id, '只回答一次');
    s.fake.reset();
    s.fake.enqueue('summary', { json: { stance: '只有立場' } }, { json: { stance: '只有立場' } });
    const end = await st.post(`/api/conversations/${conv.id}/end`);
    assert.equal(end.status, 202);
    const failed = await s.waitSummary(st, conv.id, 'failed');
    assert.equal(failed.stance, null);
    assert.equal(s.fake.of('summary').length, 2);

    const retry = await st.post(`/api/conversations/${conv.id}/summary/retry`);
    assert.equal(retry.status, 202);
    assert.equal((await s.waitSummary(st, conv.id)).status, 'ready');
    const again = await st.post(`/api/conversations/${conv.id}/summary/retry`);
    assert.equal(again.status, 409);
    assert.equal(again.json.error.code, 'summary_not_retryable');
  });

  test('AI 完全無法回應時總結失敗且不遺失對話；失敗的總結不會出現在教師清單', async () => {
    const st = await s.student();
    const t = await s.teacher();
    const conv = await s.startConversation(st);
    await s.turn(st, conv.id, '內容');
    s.fake.enqueue('summary', { status: 500 }, { status: 500 });
    await st.post(`/api/conversations/${conv.id}/end`);
    await s.waitSummary(st, conv.id, 'failed');
    assert.equal((await st.get(`/api/conversations/${conv.id}`)).json.messages.length, 2);
    assert.ok(!(await t.get('/api/teacher/summaries')).json.some((r) => r.conversation_id === conv.id));
  });

  test('沒有總結的對話查總結 → 404；教師總結清單只有教師／管理者可看', async () => {
    const st = await s.student();
    const conv = await s.startConversation(st);
    assert.equal((await st.get(`/api/conversations/${conv.id}/summary`)).status, 404);
    assert.equal((await st.get('/api/teacher/summaries')).status, 403);
    assert.equal((await s.client().get('/api/teacher/summaries')).status, 401);
  });
});

describe('個人與班級儀表板（S-04.2、S-02.3）', () => {
  let s;
  before(async () => { s = await startStack(); });
  after(async () => { await s?.stop(); });

  // 班級統計會彙整整個系統的總結，所以班級相關測試用乾淨的環境（見最後一個 describe）

  test('個人儀表板：場次、回合、學派分布與 6 維平均', async () => {
    const st = await s.student();
    const zero = (await st.get('/api/dashboard/me')).json;
    assert.deepEqual([zero.total_conversations, zero.completed, zero.radar.scored], [0, 0, 0]);

    s.fake.enqueue('summary', summaryOf('utilitarianism', scores({ utilitarianism: 5, deontology: 1 })));
    await s.completeConversation(st, { turns: 2 });
    s.fake.enqueue('summary', summaryOf('deontology', scores({ utilitarianism: 1, deontology: 5, care: 2 })));
    await s.completeConversation(st, { turns: 1 });
    await s.startConversation(st); // 進行中，不計入完成

    const me = (await st.get('/api/dashboard/me')).json;
    assert.equal(me.total_conversations, 3);
    assert.equal(me.total_turns, 3);
    assert.equal(me.completed, 2);
    assert.equal(me.frameworks.utilitarianism, 1);
    assert.equal(me.frameworks.deontology, 1);
    assert.equal(me.frameworks.unclassified, 0);
    assert.equal(me.radar.max, 5);
    assert.equal(me.radar.scored, 2);
    assert.deepEqual(me.radar.average, { ...scores({}), utilitarianism: 3, deontology: 3, care: 1 });
    assert.equal(me.recent.length, 2);
    assert.equal(me.recent[0].framework === 'deontology', true, '最近完成的在前');
  });

  test('個人儀表板只有學生本人；其他角色被擋', async () => {
    const t = await s.teacher();
    assert.equal((await t.get('/api/dashboard/me')).status, 403);
    assert.equal((await s.client().get('/api/dashboard/me')).status, 401);
  });
});

describe('班級儀表板（教師）', () => {
  let s;
  before(async () => { s = await startStack(); });
  after(async () => { await s?.stop(); });

  test('班級儀表板：全班學派分布、參與人數、主張分組與理由；學生不能看', async () => {
    const t = await s.teacher();
    const students = await Promise.all([s.student(), s.student(), s.student()]);
    s.fake.enqueue('summary', summaryOf('utilitarianism', scores({ utilitarianism: 4 }), '該拉桿'));
    await s.completeConversation(students[0]);
    s.fake.enqueue('summary', summaryOf('utilitarianism', scores({ utilitarianism: 2 }), '該拉桿'));
    await s.completeConversation(students[1]);
    s.fake.enqueue('summary', summaryOf('deontology', scores({ deontology: 5 }), '不拉桿'));
    await s.completeConversation(students[2]);

    const cls = await until(async () => {
      const r = (await t.get('/api/dashboard/class')).json;
      const claims = r.topics.find((x) => x.title === '電車難題')?.claims ?? [];
      return claims.reduce((n, c) => n + c.count, 0) === 3 ? r : false;
    }, { what: '主張分組完成' });
    assert.equal(cls.masked, false);
    assert.equal(cls.students_participating, 3);
    assert.ok(cls.students_total >= 3);
    assert.equal(cls.completed, 3);
    assert.equal(cls.frameworks.utilitarianism, 2);
    assert.equal(cls.frameworks.deontology, 1);
    assert.equal(cls.radar.average.utilitarianism, 2, '(4+2+0)/3');
    const topic = cls.topics.find((x) => x.title === '電車難題');
    assert.deepEqual(topic.claims.map((c) => [c.name, c.count]).sort(), [['不拉桿', 1], ['該拉桿', 2]]);
    assert.ok(topic.claims.every((c) => c.reasons.length > 0));

    assert.equal((await students[0].get('/api/dashboard/class')).status, 403);
    assert.equal((await s.client().get('/api/dashboard/class')).status, 401);
  });

  test('管理者看班級儀表板：內容全部遮蔽（masked）', async () => {
    const admin = await s.admin();
    const cls = (await admin.get('/api/dashboard/class')).json;
    assert.equal(cls.masked, true);
    assert.equal(cls.completed, 0);
    assert.deepEqual(cls.topics, []);
  });

  test('重新分組：AI 失敗 → 502 ai_unavailable 且原分組不變；成功後依新分組；沒有主張的題目 409；管理者不可', async () => {
    const t = await s.teacher();
    const admin = await s.admin();
    const countGroups = async () => (await t.get('/api/dashboard/class')).json.topics.find((x) => x.title === '電車難題').claims.length;
    const before = await countGroups();
    assert.ok(before >= 2);

    s.fake.enqueue('regroup', { status: 500 });
    const fail = await t.post('/api/dashboard/class/regroup', { title: '電車難題' });
    assert.equal(fail.status, 502);
    assert.equal(fail.json.error.code, 'ai_unavailable');
    assert.equal(await countGroups(), before);

    s.fake.enqueue('regroup', { json: { groups: [{ name: '全部同意', members: [1, 2, 3] }] } });
    assert.equal((await t.post('/api/dashboard/class/regroup', { title: '電車難題' })).status, 204);
    const claims = (await t.get('/api/dashboard/class')).json.topics.find((x) => x.title === '電車難題').claims;
    assert.deepEqual(claims.map((c) => [c.name, c.count]), [['全部同意', 3]]);

    assert.equal((await t.post('/api/dashboard/class/regroup', { title: '沒有這個題目' })).json.error.code, 'no_claims');
    assert.equal((await admin.post('/api/dashboard/class/regroup', { title: '電車難題' })).status, 403);
    const st = await s.student();
    assert.equal((await st.post('/api/dashboard/class/regroup', { title: '電車難題' })).status, 403);
  });
});
