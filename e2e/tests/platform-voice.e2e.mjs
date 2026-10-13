// S-08.1 同網域部署與統一錯誤格式、S-05.3／S-05.4 語音備援（經後端代理、不保存音訊）。
import { describe, test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { startStack } from '../lib/stack.mjs';

let s;
before(async () => { s = await startStack({ frontend: true }); });
after(async () => { await s?.stop(); });

describe('S-08.1 同網域：API 在 /api，前端建置產物由後端提供', () => {
  test('/health 存活檢查', async () => {
    const r = await s.client().get('/health');
    assert.equal(r.status, 200);
    assert.deepEqual(r.json, { status: 'ok' });
  });

  test('前端路由退回 index.html；靜態檔照常提供', async () => {
    const page = await s.client().get('/dashboard/some/route');
    assert.equal(page.status, 200);
    assert.ok(page.text.includes('socrates-e2e-index'));
    assert.ok((await s.client().get('/')).text.includes('socrates-e2e-index'));
    const js = await s.client().get('/app.js');
    assert.equal(js.text, 'console.log("e2e")');
  });

  test('/api 底下找不到的路徑一律回 JSON 404（不退回 index.html）', async () => {
    for (const p of ['/api/nope', '/api/conversations/x/y/z', '/api']) {
      const r = await s.client().get(p);
      assert.equal(r.status, 404, p);
      assert.equal(r.json.error.code, 'not_found');
      assert.ok(!r.text.includes('socrates-e2e-index'));
    }
  });

  test('錯誤統一為 JSON {error:{code,request_id}}，request ID 與 x-request-id 標頭一致且每次不同；不含堆疊或內部資訊', async () => {
    const st = await s.student();
    const a = await s.client().get('/api/me');
    const b = await s.client().get('/api/me');
    assert.deepEqual(Object.keys(a.json), ['error']);
    assert.deepEqual(Object.keys(a.json.error).sort(), ['code', 'request_id']);
    assert.equal(a.json.error.request_id, a.headers.get('x-request-id'));
    assert.notEqual(a.json.error.request_id, b.json.error.request_id);
    assert.ok(a.headers.get('x-request-id'));

    // 各種錯誤路徑都不洩漏內部資訊
    const bodies = [
      (await st.get('/api/conversations/not-a-uuid')).text,
      (await st.get('/api/conversations/00000000-0000-0000-0000-000000000000')).text,
      (await st.post('/api/conversations', { topic_id: '00000000-0000-0000-0000-000000000000' })).text,
      (await st.request('POST', '/api/conversations', { body: '{bad json', headers: { 'content-type': 'application/json' } })).text,
    ];
    for (const b of bodies) {
      assert.ok(!/panic|stack|backtrace|sqlx|postgres|\.rs:\d+|SELECT |Error\(/i.test(b), `不應洩漏內部資訊：${b.slice(0, 120)}`);
    }
    // CSRF 拒絕同樣帶 request ID
    const csrf = await st.post('/api/auth/logout', undefined, { origin: 'https://evil.example' });
    assert.equal(csrf.status, 403);
    assert.ok(csrf.json.error.request_id);
  });

  test('後端日誌為單純文字輸出且不含資料庫連線字串', () => {
    assert.ok(!s.logs().includes(s.dbUrl));
  });
});

describe('S-05.3／S-05.4 語音備援（OpenAI，經後端代理）', () => {
  const audio = (st, q, body = Buffer.from('AUDIO-MARKER-bytes'), type = 'audio/webm;codecs=opus') =>
    st.request('POST', `/api/voice/transcribe${q}`, { body, headers: { 'content-type': type } });

  test('只有學生可用；未登入 401、教師 403', async () => {
    const t = await s.teacher();
    assert.equal((await audio(s.client(), '?language=zh-TW')).status, 401);
    assert.equal((await audio(t, '?language=zh-TW')).status, 403);
    assert.equal((await s.client().post('/api/voice/speech', { text: 'a', language: 'en' })).status, 401);
  });

  test('辨識：音訊轉送 OpenAI、回傳逐字稿；辨識語言跟隨對話語言；音訊不寫入日誌', async () => {
    const st = await s.student();
    s.fake.reset();
    for (const [lang, iso] of [['zh-TW', 'zh'], ['en', 'en'], ['es', 'es']]) {
      const r = await audio(st, `?language=${lang}`);
      assert.equal(r.status, 200, lang);
      assert.deepEqual(r.json, { text: '這是辨識出的逐字稿' });
      const sent = s.fake.of('stt').at(-1).raw.toString('latin1');
      assert.ok(sent.includes('AUDIO-MARKER-bytes'));
      assert.ok(sent.includes(`name="language"\r\n\r\n${iso}\r\n`), `language=${iso}`);
      assert.ok(sent.includes('name="model"\r\n\r\ne2e-stt'));
    }
    assert.ok(!s.logs().includes('AUDIO-MARKER'), '原始音訊不得出現在日誌');
  });

  test('辨識：語言不支援、缺少音訊內容、非音訊類型 → 400', async () => {
    const st = await s.student();
    assert.equal((await audio(st, '?language=fr')).json.error.code, 'invalid_language');
    assert.equal((await audio(st, '?language=en', Buffer.alloc(0))).json.error.code, 'invalid_audio');
    assert.equal((await audio(st, '?language=en', Buffer.from('x'), 'text/plain')).json.error.code, 'invalid_audio');
  });

  test('辨識：OpenAI 失敗 → 502 voice_failed（前端改提示文字輸入）', async () => {
    const st = await s.student();
    s.fake.enqueue('stt', { status: 500 });
    const r = await audio(st, '?language=en');
    assert.equal(r.status, 502);
    assert.equal(r.json.error.code, 'voice_failed');
  });

  test('合成：回傳 mp3；文字長度 1–4000；語言需支援；OpenAI 失敗 → 502', async () => {
    const st = await s.student();
    const ok = await st.post('/api/voice/speech', { text: '  你好  ', language: 'zh-TW' });
    assert.equal(ok.status, 200);
    assert.equal(ok.headers.get('content-type'), 'audio/mpeg');
    assert.equal(ok.buffer.toString(), 'ID3-fake-mp3');
    assert.equal(s.fake.of('tts').at(-1).body.input, '你好');
    assert.equal(s.fake.of('tts').at(-1).body.model, 'e2e-tts');

    assert.equal((await st.post('/api/voice/speech', { text: '   ', language: 'en' })).json.error.code, 'invalid_text');
    assert.equal((await st.post('/api/voice/speech', { text: 'x'.repeat(4001), language: 'en' })).json.error.code, 'invalid_text');
    assert.equal((await st.post('/api/voice/speech', { text: 'x'.repeat(4000), language: 'es' })).status, 200);
    assert.equal((await st.post('/api/voice/speech', { text: 'hi', language: 'fr' })).json.error.code, 'invalid_language');
    s.fake.enqueue('tts', { status: 500 });
    assert.equal((await st.post('/api/voice/speech', { text: 'hi', language: 'en' })).status, 502);
  });

  test('語音逐字稿以來源 speech-openai／speech-browser 併入對話紀錄（S-05.4），不保存音訊', async () => {
    const st = await s.student();
    const conv = await s.startConversation(st);
    const transcript = (await audio(st, '?language=zh-TW')).json.text;
    await s.turn(st, conv.id, transcript, { source: 'speech-openai' });
    await s.turn(st, conv.id, '瀏覽器辨識的文字', { source: 'speech-browser' });
    const msgs = (await st.get(`/api/conversations/${conv.id}`)).json.messages.filter((m) => m.role === 'student');
    assert.deepEqual(msgs.map((m) => m.source), ['speech-openai', 'speech-browser']);
    assert.equal(s.sql(`SELECT count(*) FROM information_schema.columns WHERE table_schema = 'public' AND data_type = 'bytea' AND table_name NOT IN ('sessions', '_sqlx_migrations')`), '0', '資料庫沒有任何可存音訊的欄位');
  });
});
