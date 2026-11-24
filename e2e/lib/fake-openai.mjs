// 假的 OpenAI 相容伺服器：讓 e2e 在不呼叫真實 AI 的情況下，決定性地驅動後端（S-03.1 單一介面、S-03.5 失敗行為）。
//
// 後端只會呼叫三個端點：/chat/completions（串流）、/audio/transcriptions、/audio/speech。
// 以 system prompt 的內容區分「對話追問 / 總結 / 主張分組 / 重新分組」。
import http from 'node:http';

export const META_DELIM = '<<<META>>>';

/** 一般追問回覆：先給學生看的文字，再接結構化段落。 */
export function chatReply(text, meta = {}) {
  const m = { question_type: 'clarify', advance: false, reason: 'e2e', ...meta };
  const cut = Math.ceil(text.length / 2);
  return { chunks: [text.slice(0, cut), text.slice(cut), `\n${META_DELIM}\n${JSON.stringify(m)}`] };
}

export const DEFAULT_SUMMARY = {
  stance: '支持拉桿',
  reasons: '五條人命比一條人命重要',
  claim: '該拉桿',
  framework: 'utilitarianism',
  scores: { utilitarianism: 5, deontology: 1, virtue: 2, contractarianism: 0, care: 1, existentialism: 0 },
};

function kindOf(system) {
  if (system.includes('紀錄整理者')) return 'summary';
  if (system.includes('以下是每位學生最終主張的一句短句')) return 'regroup';
  if (system.includes('只有真的不屬於任何既有群組時')) return 'group';
  if (system.includes('蘇格拉底式')) return 'chat';
  return 'unknown';
}

export async function startFakeOpenAi() {
  /** 每個請求的紀錄（供斷言用）。 */
  const requests = [];
  /** 依種類排隊的行為；用完後走預設。 */
  const queues = { chat: [], summary: [], group: [], regroup: [], stt: [], tts: [] };
  const defaults = {
    chat: () => chatReply('你所說的「公平」是什麼意思？'),
    summary: () => ({ json: DEFAULT_SUMMARY }),
    group: (req) => ({ json: { group: req.messages.at(-1)?.content ?? '其他' } }),
    regroup: () => ({ status: 500 }),
    stt: () => ({ json: { text: '這是辨識出的逐字稿' } }),
    tts: () => ({ audio: Buffer.from('ID3-fake-mp3') }),
  };
  const fake = {
    requests,
    url: '',
    apiKey: 'sk-e2e-secret-key',
    /** 下一次（依序）該種類請求的行為：{ chunks } | { json } | { status } | { hang } | { chunks, breakAfter } | { chunks, stallAfter } */
    enqueue(kind, ...behaviors) { queues[kind].push(...behaviors); },
    setDefault(kind, fn) { defaults[kind] = fn; },
    of(kind) { return requests.filter((r) => r.kind === kind); },
    reset() { requests.length = 0; for (const q of Object.values(queues)) q.length = 0; },
    close: () => new Promise((resolve) => { server.closeAllConnections?.(); server.close(resolve); }),
  };

  const server = http.createServer(async (req, res) => {
    const chunks = [];
    for await (const c of req) chunks.push(c);
    const raw = Buffer.concat(chunks);
    const url = new URL(req.url, 'http://x');
    if (req.headers.authorization !== `Bearer ${fake.apiKey}`) {
      res.writeHead(401).end('bad key');
      return;
    }

    if (url.pathname === '/v1/chat/completions') {
      const body = JSON.parse(raw.toString('utf8'));
      const system = body.messages.find((m) => m.role === 'system')?.content ?? '';
      const kind = kindOf(system);
      const rec = { kind, system, messages: body.messages.filter((m) => m.role !== 'system'), model: body.model, stream: body.stream };
      requests.push(rec);
      const behavior = queues[kind]?.shift() ?? defaults[kind]?.(rec) ?? { status: 500 };
      return respondChat(res, behavior);
    }
    if (url.pathname === '/v1/audio/transcriptions') {
      requests.push({ kind: 'stt', bytes: raw.length, contentType: req.headers['content-type'], raw });
      const b = queues.stt.shift() ?? defaults.stt();
      if (b.status) return void res.writeHead(b.status).end('err');
      return void res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify(b.json));
    }
    if (url.pathname === '/v1/audio/speech') {
      const body = JSON.parse(raw.toString('utf8'));
      requests.push({ kind: 'tts', body });
      const b = queues.tts.shift() ?? defaults.tts();
      if (b.status) return void res.writeHead(b.status).end('err');
      return void res.writeHead(200, { 'content-type': 'audio/mpeg' }).end(b.audio);
    }
    res.writeHead(404).end();
  });

  function respondChat(res, b) {
    if (b.status) return void res.writeHead(b.status).end('upstream error');
    if (b.hang) return; // 永不回應：用來測逾時
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    const parts = b.json ? [JSON.stringify(b.json)] : b.chunks;
    const send = (i) => {
      if (b.breakAfter !== undefined && i >= b.breakAfter) return void res.destroy(); // 串流中途斷線
      if (b.stallAfter !== undefined && i >= b.stallAfter) return; // 送出部分內容後停住（完整回覆逾時）
      if (i >= parts.length) {
        res.write('data: {"choices":[{"delta":{},"finish_reason":"stop"}]}\n\n');
        res.write('data: [DONE]\n\n');
        return void res.end();
      }
      res.write(`data: ${JSON.stringify({ choices: [{ delta: { content: parts[i] } }] })}\n\n`);
      setTimeout(() => send(i + 1), 15);
    };
    send(0);
  }

  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  fake.url = `http://127.0.0.1:${server.address().port}/v1`;
  return fake;
}
