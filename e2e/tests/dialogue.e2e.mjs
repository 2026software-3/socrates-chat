// S-03.2 追問階段、S-03.3 回合上限與收斂、S-03.5 AI 失敗行為、S-08.4 SSE。
// 本檔的後端以 WRAP_UP_TURN=3、MAX_TURNS=5、首 token 1 秒、完整回覆 3 秒啟動（預設值另見 dialogue-defaults）。
import { describe, test, before, after, beforeEach } from 'node:test';
import assert from 'node:assert/strict';
import { startStack, until, sleep, uniq } from '../lib/stack.mjs';
import { chatReply, META_DELIM } from '../lib/fake-openai.mjs';

let s;
let st;
before(async () => { s = await startStack(); });
after(async () => { await s?.stop(); });
beforeEach(async () => {
  s.fake.reset();
  st = await s.student();
});

const deltas = (stream) => stream.events.filter((e) => e.event === 'delta').map((e) => e.data.text).join('').trim();
const last = (stream) => stream.events.at(-1);

describe('S-03.2 / S-08.4 一個回合：存學生訊息 → SSE 串流 AI 回覆', () => {
  test('學生訊息先保存（201），再以 SSE 串流取得 AI 回覆；結構化段落不會給學生看', async () => {
    const conv = await s.startConversation(st);
    assert.deepEqual({ status: conv.status, stage: conv.stage, turns: conv.turn_count, lang: conv.language }, { status: 'active', stage: 1, turns: 0, lang: 'zh-TW' });
    s.fake.enqueue('chat', chatReply('你說的「公平」是什麼意思？', { question_type: 'clarify', reason: 'REASON-SECRET-123' }));

    const m = await st.post(`/api/conversations/${conv.id}/messages`, { content: '  我認為應該拉桿  ' });
    assert.equal(m.status, 201);
    assert.equal(m.json.role, 'student');
    assert.equal(m.json.content, '我認為應該拉桿', '去除前後空白');
    assert.equal(m.json.source, 'text');
    assert.equal(m.json.auto_ended, false);

    const stream = await st.sse(`/api/conversations/${conv.id}/stream`);
    assert.equal(stream.status, 200);
    assert.match(stream.contentType, /^text\/event-stream/);
    assert.equal(deltas(stream), '你說的「公平」是什麼意思？');
    assert.ok(stream.events.filter((e) => e.event === 'delta').length >= 2, '逐段串流');
    assert.ok(!stream.text.includes(META_DELIM), '分隔標記與 JSON 不給學生看');
    const done = last(stream);
    assert.equal(done.event, 'done');
    assert.equal(done.data.question_type, 'clarify');
    assert.equal(done.data.stage, 1);
    assert.equal(done.data.suggest_end, false);

    const detail = (await st.get(`/api/conversations/${conv.id}`)).json;
    assert.equal(detail.turn_count, 1);
    assert.deepEqual(detail.messages.map((x) => x.role), ['student', 'ai']);
    assert.equal(detail.messages[1].content, '你說的「公平」是什麼意思？');
    assert.equal(detail.messages[1].question_type, 'clarify');
    assert.ok(!JSON.stringify(detail).includes('REASON-SECRET-123'), '階段判斷理由只供稽核，不回傳給學生');
    assert.ok(!JSON.stringify(detail).includes(META_DELIM));
    assert.equal(s.sql(`SELECT advance_reason FROM messages WHERE id = '${done.data.message_id}'`), 'REASON-SECRET-123');
    assert.equal(s.sql(`SELECT rules_version FROM messages WHERE id = '${done.data.message_id}'`), 'v1/zh-TW', '規則版本記錄在每則 AI 回覆上');
  });

  test('送給 AI 的提示：題目、蘇格拉底式規則（一次一問、不代答、中立、離題帶回）、目前階段、完整對話歷史', async () => {
    const conv = await s.startConversation(st);
    await s.turn(st, conv.id, '第一句話');
    await s.turn(st, conv.id, '第二句話');
    const [r1, r2] = s.fake.of('chat');
    assert.equal(r1.model, 'e2e-model');
    assert.equal(r1.stream, true);
    for (const needle of ['電車難題', '每次回覆只問「一個」問題', '絕不提出你自己的答案或立場', '你覺得呢', '中立、尊重', '離題', '第 1 階段「釐清」']) {
      assert.ok(r1.system.includes(needle), `提示應包含：${needle}`);
    }
    assert.deepEqual(r1.messages, [{ role: 'user', content: '第一句話' }]);
    assert.deepEqual(r2.messages.map((m) => [m.role, m.content.slice(0, 5)]), [['user', '第一句話'], ['assistant', '你所說的「'], ['user', '第二句話']]);
  });

  test('AI 金鑰只存在後端：以 Bearer 傳給 AI 服務，不出現在任何回應', async () => {
    const conv = await s.startConversation(st);
    const t = await s.turn(st, conv.id, 'hello');
    // fake 會對金鑰不符回 401，所以能成功就代表後端帶了正確的金鑰
    assert.equal(last(t.stream).event, 'done');
    const all = [JSON.stringify(t.message.json), t.stream.text, JSON.stringify((await st.get(`/api/conversations/${conv.id}`)).json)].join('\n');
    assert.ok(!all.includes(s.fake.apiKey));
    assert.ok(!s.logs().includes(s.fake.apiKey));
  });
});

describe('S-03.2 三階段只進不退', () => {
  test('釐清 → 論證 → 挑戰，advance=false 不退回；第 3 階段達成條件即符合收斂條件', async () => {
    const conv = await s.startConversation(st);
    const plan = [
      { advance: true, stage: 2, ready: false },   // 回合 1：釐清達成 → 進入論證
      { advance: false, stage: 2, ready: false },  // 回合 2：留在論證
      { advance: true, stage: 3, ready: false },   // 回合 3：論證達成 → 進入挑戰（尚未收斂）
      { advance: false, stage: 3, ready: false },  // 回合 4：挑戰中，沒有退回
    ];
    for (const [i, p] of plan.entries()) {
      s.fake.enqueue('chat', chatReply(`問題 ${i + 1}`, { advance: p.advance, question_type: 'reason' }));
      const t = await s.turn(st, conv.id, `回答 ${i + 1}`);
      assert.equal(last(t.stream).event, 'done', `回合 ${i + 1}`);
      assert.equal(last(t.stream).data.stage, p.stage, `回合 ${i + 1} 的階段`);
      assert.equal(last(t.stream).data.suggest_end, p.ready, `回合 ${i + 1} 的收斂`);
    }
    // 每回合提示裡的階段 = 後端記錄的階段，而不是模型自己記憶的
    const stages = s.fake.of('chat').map((r) => /第 ([123]) 階段/.exec(r.system)[1]);
    assert.deepEqual(stages, ['1', '2', '2', '3']);
    assert.ok(s.fake.of('chat')[3].system.includes('反例或對立觀點'));
    assert.equal((await st.get(`/api/conversations/${conv.id}`)).json.stage, 3);
  });

  test('第 3 階段學生回應過反例／對立觀點（advance=true）→ converge_ready，AI 可提議收尾', async () => {
    const conv = await s.startConversation(st);
    s.sql(`UPDATE conversations SET stage = 3 WHERE id = '${conv.id}'`);
    s.fake.enqueue('chat', chatReply('要不要整理結論？', { advance: true, question_type: 'wrap_up' }));
    const t = await s.turn(st, conv.id, '我回應了反例');
    const done = last(t.stream);
    assert.equal(done.data.stage, 3, '不會超過第 3 階段');
    assert.equal(done.data.suggest_end, true);
    assert.equal((await st.get(`/api/conversations/${conv.id}`)).json.converge_ready, true);
    // 一旦符合就維持符合
    s.fake.enqueue('chat', chatReply('再問一題', { advance: false }));
    const again = await s.turn(st, conv.id, '補充');
    assert.equal(last(again.stream).data.suggest_end, true);
  });

  test('結構化段落格式不符或缺少 → 保守處理：不晉級、類型未知，回覆文字照常保存', async () => {
    const conv = await s.startConversation(st);
    const cases = [
      { chunks: ['純文字，沒有結構化段落'] },
      { chunks: ['壞掉的 JSON', `\n${META_DELIM}\n{not json`] },
      { chunks: ['未知類型', `\n${META_DELIM}\n{"question_type":"hacked","advance":false}`] },
    ];
    for (const [i, c] of cases.entries()) {
      s.fake.enqueue('chat', c);
      const t = await s.turn(st, conv.id, `回合 ${i}`);
      const done = last(t.stream);
      assert.equal(done.event, 'done', `case ${i}`);
      assert.equal(done.data.question_type, null);
      assert.equal(done.data.stage, 1);
    }
  });

  test('模型即使在第 1 階段就回報 advance=true，一次也只前進一個階段', async () => {
    const conv = await s.startConversation(st);
    s.fake.enqueue('chat', chatReply('下一題', { advance: true }));
    const t = await s.turn(st, conv.id, '一口氣全說完');
    assert.equal(last(t.stream).data.stage, 2);
  });
});

describe('S-03.3 回合上限', () => {
  test(`第 WRAP_UP_TURN(=3) 回合起提示 AI 引導收尾，之前不會`, async () => {
    const conv = await s.startConversation(st);
    for (let i = 1; i <= 4; i++) await s.turn(st, conv.id, `回答 ${i}`);
    const hasHint = s.fake.of('chat').map((r) => r.system.includes('請開始引導學生整理自己的結論'));
    assert.deepEqual(hasHint, [false, false, true, true]);
  });

  test('第 MAX_TURNS(=5) 回合：不再追問，直接產生總結並結束', async () => {
    const conv = await s.startConversation(st);
    for (let i = 1; i <= 4; i++) await s.turn(st, conv.id, `回答 ${i}`);
    const chatBefore = s.fake.of('chat').length;

    const m = await st.post(`/api/conversations/${conv.id}/messages`, { content: '最後一句' });
    assert.equal(m.status, 201);
    assert.equal(m.json.auto_ended, true);
    const detail = (await st.get(`/api/conversations/${conv.id}`)).json;
    assert.equal(detail.status, 'ended');
    assert.equal(detail.turn_count, 5);
    assert.ok(detail.ended_at);

    const summary = await s.waitSummary(st, conv.id);
    assert.equal(summary.status, 'ready');
    assert.equal(s.fake.of('chat').length, chatBefore, '第 5 回合 AI 不再追問');
    assert.equal(s.fake.of('summary').length, 1);

    const after = await st.post(`/api/conversations/${conv.id}/messages`, { content: '還想講' });
    assert.equal(after.status, 409);
    assert.equal(after.json.error.code, 'conversation_ended');
    const stream = await st.sse(`/api/conversations/${conv.id}/stream`);
    assert.equal(stream.status, 409);
    assert.equal(stream.json.error.code, 'conversation_ended');
  });

  test('學生任何時候都可以主動結束；還沒有任何討論時不能結束', async () => {
    const conv = await s.startConversation(st);
    const empty = await st.post(`/api/conversations/${conv.id}/end`);
    assert.equal(empty.status, 409);
    assert.equal(empty.json.error.code, 'empty_conversation');

    await s.turn(st, conv.id, '只講了一回合');
    const end = await st.post(`/api/conversations/${conv.id}/end`);
    assert.equal(end.status, 202);
    assert.equal(end.json.status, 'pending');
    const done = await s.waitSummary(st, conv.id);
    assert.equal(done.status, 'ready');
    assert.equal((await st.get(`/api/conversations/${conv.id}`)).json.status, 'ended');

    const again = await st.post(`/api/conversations/${conv.id}/end`);
    assert.equal(again.status, 200, '重複結束是冪等的');
    assert.equal(again.json.status, 'ready');
    assert.equal(s.fake.of('summary').length, 1, '不會重複產生總結');
  });

  test('訊息驗證：不可為空、上限 4000 字元、來源只能是 text／speech-browser／speech-openai', async () => {
    const conv = await s.startConversation(st);
    const send = (body) => st.post(`/api/conversations/${conv.id}/messages`, body);
    assert.equal((await send({ content: '   ' })).json.error.code, 'invalid_message');
    assert.equal((await send({ content: 'x'.repeat(4001) })).json.error.code, 'invalid_message');
    assert.equal((await send({ content: '好', source: 'telepathy' })).json.error.code, 'invalid_source');
    assert.equal((await st.get(`/api/conversations/${conv.id}`)).json.turn_count, 0, '驗證失敗不算一個回合');

    // 通過驗證的三種來源（S-05.4 記錄來源）；每則之間讓 AI 回覆完畢
    for (const source of ['text', 'speech-browser', 'speech-openai']) {
      const r = await send({ content: `來源 ${source}`, source });
      assert.equal(r.status, 201);
      assert.equal(r.json.source, source);
      await st.sse(`/api/conversations/${conv.id}/stream`);
    }
    const long = await send({ content: 'y'.repeat(4000) });
    assert.equal(long.status, 201, '剛好 4000 字元可以');
  });
});

describe('S-03.5 外部 AI 失敗行為', () => {
  test('AI 完全失敗：自動重試 1 次後顯示錯誤；學生訊息不遺失；按重試即可補回覆（不多算回合）', async () => {
    const conv = await s.startConversation(st);
    s.fake.enqueue('chat', { status: 500 }, { status: 500 });
    const t = await s.turn(st, conv.id, '這句話不能遺失');
    assert.equal(t.message.status, 201, '學生訊息先存下來');
    assert.deepEqual(t.stream.events, [{ event: 'error', data: { code: 'ai_unavailable' } }]);
    assert.equal(s.fake.of('chat').length, 2, '自動重試 1 次（共 2 次呼叫）');

    const mid = (await st.get(`/api/conversations/${conv.id}`)).json;
    assert.deepEqual(mid.messages.map((m) => [m.role, m.content]), [['student', '這句話不能遺失']]);
    assert.equal(mid.turn_count, 1);

    // 重試按鈕 = 再開一次串流
    const retry = await st.sse(`/api/conversations/${conv.id}/stream`);
    assert.equal(last(retry).event, 'done');
    const after = (await st.get(`/api/conversations/${conv.id}`)).json;
    assert.deepEqual(after.messages.map((m) => m.role), ['student', 'ai']);
    assert.equal(after.turn_count, 1, '重試不算新的回合');
  });

  test('第一次尚未串流出任何內容就失敗 → 自動重試成功，學生看不到錯誤也不會重複內容', async () => {
    const conv = await s.startConversation(st);
    s.fake.enqueue('chat', { status: 503 }, chatReply('重試後的回覆'));
    const t = await s.turn(st, conv.id, '你好');
    assert.equal(last(t.stream).event, 'done');
    assert.ok(!t.stream.events.some((e) => e.event === 'error'));
    assert.equal(deltas(t.stream), '重試後的回覆');
    assert.equal(s.fake.of('chat').length, 2);
  });

  test('串流中途失敗：捨棄部分內容、標記失敗、不自動重試；之後可手動重試', async () => {
    const conv = await s.startConversation(st);
    s.fake.enqueue('chat', { chunks: ['說到一半', '就斷', '線'], breakAfter: 1 });
    const t = await s.turn(st, conv.id, '請回答');
    assert.equal(last(t.stream).event, 'error');
    assert.equal(last(t.stream).data.code, 'ai_unavailable');
    assert.equal(s.fake.of('chat').length, 1, '已串流出內容後不自動重試，避免同一段回覆出現兩次');

    const mid = (await st.get(`/api/conversations/${conv.id}`)).json;
    assert.deepEqual(mid.messages.map((m) => m.role), ['student'], '部分內容不保存');
    assert.ok(!JSON.stringify(mid).includes('說到一半'));

    s.fake.enqueue('chat', chatReply('完整的回覆'));
    const retry = await st.sse(`/api/conversations/${conv.id}/stream`);
    assert.equal(last(retry).event, 'done');
    assert.equal((await st.get(`/api/conversations/${conv.id}`)).json.messages.at(-1).content, '完整的回覆');
  });

  test('首個 token 逾時（測試環境 1 秒）：每次嘗試逾時，重試 1 次後失敗，總等待有上限', async () => {
    const conv = await s.startConversation(st);
    s.fake.enqueue('chat', { hang: true }, { hang: true });
    const t0 = Date.now();
    const t = await s.turn(st, conv.id, '會逾時');
    const elapsed = Date.now() - t0;
    assert.equal(last(t.stream).event, 'error');
    assert.equal(s.fake.of('chat').length, 2);
    assert.ok(elapsed >= 1900 && elapsed < 4500, `約 2 次逾時的時間，實際 ${elapsed}ms`);
  });

  test('完整回覆逾時（測試環境 3 秒）：中斷並標記失敗，不重試', async () => {
    const conv = await s.startConversation(st);
    s.fake.enqueue('chat', { chunks: ['開頭', '永遠', '說不完'], stallAfter: 1 });
    const t0 = Date.now();
    const t = await s.turn(st, conv.id, '會卡住');
    const elapsed = Date.now() - t0;
    assert.equal(last(t.stream).event, 'error');
    assert.equal(s.fake.of('chat').length, 1);
    assert.ok(elapsed >= 2800 && elapsed < 6000, `約完整回覆上限，實際 ${elapsed}ms`);
    assert.deepEqual((await st.get(`/api/conversations/${conv.id}`)).json.messages.map((m) => m.role), ['student']);
  });

  test('同一場對話同時只允許一條回覆串流；回覆中不接受新訊息或結束', async () => {
    const conv = await s.startConversation(st);
    s.fake.enqueue('chat', { hang: true }, { hang: true });
    await st.post(`/api/conversations/${conv.id}/messages`, { content: '第一句' });
    const first = st.sse(`/api/conversations/${conv.id}/stream`); // 約 2 秒後才會失敗
    await sleep(400);

    const second = await st.sse(`/api/conversations/${conv.id}/stream`);
    assert.equal(second.status, 409);
    assert.equal(second.json.error.code, 'reply_in_progress');
    const msg = await st.post(`/api/conversations/${conv.id}/messages`, { content: '搶著說' });
    assert.equal(msg.status, 409);
    assert.equal(msg.json.error.code, 'reply_in_progress');
    const end = await st.post(`/api/conversations/${conv.id}/end`);
    assert.equal(end.status, 409);
    assert.equal(end.json.error.code, 'reply_in_progress');

    assert.equal(last(await first).event, 'error');
    // 串流結束後釋放，可以繼續
    const retry = await st.sse(`/api/conversations/${conv.id}/stream`);
    assert.equal(last(retry).event, 'done');
    assert.equal((await st.get(`/api/conversations/${conv.id}`)).json.turn_count, 1, '被擋下的訊息不算回合');
  });

  test('沒有待回覆的學生訊息時開串流 → 409 nothing_to_reply', async () => {
    const conv = await s.startConversation(st);
    const empty = await st.sse(`/api/conversations/${conv.id}/stream`);
    assert.equal(empty.status, 409);
    assert.equal(empty.json.error.code, 'nothing_to_reply');
    await s.turn(st, conv.id, '一問一答');
    const replied = await st.sse(`/api/conversations/${conv.id}/stream`);
    assert.equal(replied.json.error.code, 'nothing_to_reply', '已經回覆過了');
  });

  test('前端中途斷線，後端仍會把完整回覆存下來（S-08.4）', async () => {
    const conv = await s.startConversation(st);
    s.fake.enqueue('chat', chatReply('這段回覆要完整保存下來'));
    await st.post(`/api/conversations/${conv.id}/messages`, { content: '我會斷線' });
    const ctrl = new AbortController();
    const res = await fetch(`${s.base}/api/conversations/${conv.id}/stream`, { headers: { cookie: `sid=${st.sid}` }, signal: ctrl.signal });
    const reader = res.body.getReader();
    await reader.read();
    ctrl.abort();
    const saved = await until(async () => {
      const d = (await st.get(`/api/conversations/${conv.id}`)).json;
      return d.messages.length === 2 ? d : false;
    }, { what: 'AI 回覆被保存' });
    assert.equal(saved.messages[1].content, '這段回覆要完整保存下來');
  });
});

describe('S-03.3 / S-03.2 與 AI 失敗交錯', () => {
  test('回合上限的自動結束不依賴 AI 追問成功', async () => {
    const conv = await s.startConversation(st);
    for (let i = 1; i <= 4; i++) await s.turn(st, conv.id, `回答 ${i}`);
    const m = await st.post(`/api/conversations/${conv.id}/messages`, { content: `${uniq('last')}` });
    assert.equal(m.json.auto_ended, true);
    assert.equal((await s.waitSummary(st, conv.id)).status, 'ready');
  });
});
