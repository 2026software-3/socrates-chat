// 以「未設定任何選填環境變數」啟動，驗證規格預設值：回合 15／20（S-03.3）、cookie Secure（S-08.2 §4）、語音備援未設定（S-05.3）。
import { describe, test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { startStack, ADMIN2_EMAIL, ADMIN_INITIAL_PASSWORD } from '../lib/stack.mjs';

let s;
before(async () => {
  s = await startStack({
    speech: false,
    env: { WRAP_UP_TURN: null, MAX_TURNS: null, AI_FIRST_TOKEN_TIMEOUT_SECS: null, AI_TOTAL_TIMEOUT_SECS: null, COOKIE_SECURE: null },
  });
});
after(async () => { await s?.stop(); });

describe('S-03.3 預設回合規則：第 15 回合起引導收尾、第 20 回合直接結束', () => {
  test('第 14 回合沒有收尾引導、第 15 回合有；第 20 回合自動結束並產生總結', async () => {
    const st = await s.student();
    const conv = await s.startConversation(st);
    const send = (n) => st.post(`/api/conversations/${conv.id}/messages`, { content: `第 ${n} 回合` });

    for (let n = 1; n <= 13; n++) assert.equal((await send(n)).json.auto_ended, false);
    for (const n of [14, 15]) {
      assert.equal((await send(n)).status, 201);
      assert.equal((await st.sse(`/api/conversations/${conv.id}/stream`)).events.at(-1).event, 'done');
    }
    const [at14, at15] = s.fake.of('chat').map((r) => r.system.includes('請開始引導學生整理自己的結論'));
    assert.equal(at14, false);
    assert.equal(at15, true);

    for (let n = 16; n <= 19; n++) assert.equal((await send(n)).json.auto_ended, false);
    const final = await send(20);
    assert.equal(final.json.auto_ended, true);
    assert.equal((await st.get(`/api/conversations/${conv.id}`)).json.status, 'ended');
    assert.equal((await s.waitSummary(st, conv.id)).status, 'ready');
  });
});

describe('預設設定', () => {
  test('session cookie 預設加 Secure（只有本機開發才關閉）', async () => {
    const r = await s.client().post('/api/auth/login', { email: ADMIN2_EMAIL, password: ADMIN_INITIAL_PASSWORD });
    assert.equal(r.status, 200, r.text);
    assert.match(r.setCookie.find((l) => l.startsWith('sid=')), /; Secure/i);
  });

  test('未設定語音模型時語音備援不啟用：503 voice_unavailable（前端改提示文字輸入）', async () => {
    const st = await s.student();
    const stt = await st.request('POST', '/api/voice/transcribe?language=zh-TW', { body: Buffer.from('audio'), headers: { 'content-type': 'audio/webm' } });
    assert.equal(stt.status, 503);
    assert.equal(stt.json.error.code, 'voice_unavailable');
    const tts = await st.post('/api/voice/speech', { text: '你好', language: 'zh-TW' });
    assert.equal(tts.status, 503);
    assert.equal(tts.json.error.code, 'voice_unavailable');
  });
});
