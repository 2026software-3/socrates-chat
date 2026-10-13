// 帶 cookie jar 的 HTTP client：模擬瀏覽器對後端的呼叫（同網域、自動帶 Origin）。

export class Client {
  constructor(base, { origin = base } = {}) {
    this.base = base;
    this.origin = origin;
    this.cookies = new Map();
  }

  get sid() { return this.cookies.get('sid'); }

  #cookieHeader() {
    return [...this.cookies].map(([k, v]) => `${k}=${v}`).join('; ');
  }

  #absorb(res) {
    for (const line of res.headers.getSetCookie()) {
      const [pair] = line.split(';');
      const i = pair.indexOf('=');
      const name = pair.slice(0, i).trim();
      const value = pair.slice(i + 1).trim();
      const maxAge = /max-age=(\d+)/i.exec(line);
      if (value === '' || (maxAge && Number(maxAge[1]) === 0)) this.cookies.delete(name);
      else this.cookies.set(name, value);
    }
  }

  async request(method, path, { json, body, headers = {}, origin = this.origin, noCookie = false } = {}) {
    const h = { ...headers };
    if (origin !== null) h.origin = origin;
    const cookie = noCookie ? '' : this.#cookieHeader();
    if (cookie) h.cookie = cookie;
    let payload = body;
    if (json !== undefined) {
      h['content-type'] = 'application/json';
      payload = JSON.stringify(json);
    }
    const res = await fetch(this.base + path, { method, headers: h, body: payload, redirect: 'manual' });
    this.#absorb(res);
    const buffer = Buffer.from(await res.arrayBuffer());
    const text = buffer.toString('utf8');
    let parsed;
    try { parsed = JSON.parse(text); } catch { parsed = undefined; }
    return { status: res.status, headers: res.headers, text, json: parsed, buffer, setCookie: res.headers.getSetCookie() };
  }

  get(path, opts) { return this.request('GET', path, opts); }
  post(path, json, opts) { return this.request('POST', path, { json, ...opts }); }
  patch(path, json, opts) { return this.request('PATCH', path, { json, ...opts }); }
  del(path, opts) { return this.request('DELETE', path, opts); }

  /** 開啟 SSE 串流並讀到結束，回傳事件陣列 [{event, data}]。非 200 時回傳狀態與 JSON 錯誤。 */
  async sse(path) {
    const h = { accept: 'text/event-stream' };
    const cookie = this.#cookieHeader();
    if (cookie) h.cookie = cookie;
    const res = await fetch(this.base + path, { headers: h });
    const text = await res.text();
    if (res.status !== 200) {
      let json; try { json = JSON.parse(text); } catch { /* ignore */ }
      return { status: res.status, json, events: [] };
    }
    const events = [];
    for (const block of text.split(/\n\n/)) {
      let event = 'message'; const data = [];
      for (const line of block.split('\n')) {
        if (line.startsWith('event:')) event = line.slice(6).trim();
        else if (line.startsWith('data:')) data.push(line.slice(5).trimStart());
      }
      if (data.length) {
        let d = data.join('\n');
        try { d = JSON.parse(d); } catch { /* keep text */ }
        events.push({ event, data: d });
      }
    }
    return { status: 200, events, text, contentType: res.headers.get('content-type') };
  }
}
