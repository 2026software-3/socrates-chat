/* Shared content, routing and behaviour for the interior proposals.
   Each proposal page supplies its own shell signature through window.Interior.init({ ... }). All data is synthetic. */
(function () {
  const ICONS = {
    menu: '<path d="M4 6h16M4 12h16M4 18h16"/>',
    chevron: '<path d="m6 9 6 6 6-6"/>',
    back: '<path d="m15 18-6-6 6-6"/>',
    mic: '<path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z"/><path d="M19 10v2a7 7 0 0 1-14 0v-2M12 19v3"/>',
    send: '<path d="M14.5 21.7a.5.5 0 0 0 .9-.1L22 2.6a.5.5 0 0 0-.6-.6L2.4 8.6a.5.5 0 0 0-.1.9l7.9 3.2a2 2 0 0 1 1.1 1.1z"/><path d="m21.9 2.1-10.9 10.9"/>',
    close: '<path d="M18 6 6 18M6 6l12 12"/>',
    plus: '<path d="M5 12h14M12 5v14"/>',
    search: '<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>',
  };
  const icon = (n, cls = 'ico') => `<svg class="${cls}" viewBox="0 0 24 24" aria-hidden="true">${ICONS[n]}</svg>`;

  const NAV = {
    student: [
      { id: 'home', n: 'I', zh: '我的教室', la: 'Basilica Ivlia', b: 'basilica' },
      { id: 'practice', n: 'II', zh: '個人練習', la: 'Aedes Vestae', b: 'vesta' },
      { id: 'talks', n: 'III', zh: '我的對話', la: 'Rostra', b: 'rostra' },
      { id: 'points', n: 'IV', zh: '點數與任務', la: 'Aedes Satvrni', b: 'saturn' },
      { id: 'companion', n: 'V', zh: '我的蘇格拉底', la: 'Aedes Castoris', b: 'castor' },
      { id: 'profile', n: 'VI', zh: '個人檔案', la: 'Tabvlarivm', b: 'tabularium' },
    ],
    teacher: [
      { id: 't-classes', n: 'I', zh: '教室管理', la: 'Basilica Ivlia', b: 'basilica' },
      { id: 't-topics', n: 'II', zh: '題目庫', la: 'Tabvlarivm', b: 'tabularium' },
      { id: 't-books', n: 'III', zh: '共用書庫', la: 'Aedes Satvrni', b: 'saturn' },
      { id: 't-students', n: 'IV', zh: '學生帳號', la: 'Cvria', b: 'curia' },
      { id: 't-summaries', n: 'V', zh: '學生總結', la: 'Rostra', b: 'rostra' },
    ],
  };
  const PEOPLE = {
    student: { name: '王小明', initial: '王', role: '學生', sub: '哲學概論・週二班', points: 128 },
    teacher: { name: '林老師', initial: '林', role: '教師', sub: '2 個教室', points: null },
  };
  const ACTIVITIES = [
    { tav: 'XII', title: '民主一定比較公平嗎？', cls: '哲學概論・週二班', due: '11 月 14 日（五）截止', status: 'live', state: 2, rounds: 5, cta: '繼續對話', plate: 2 },
    { tav: 'XIII', title: '說謊永遠是錯的嗎？', cls: '哲學概論・週二班', due: '11 月 21 日（五）截止', status: 'new', state: 0, rounds: 0, cta: '開始對話', plate: 4 },
    { tav: 'XI', title: '知識就是力量嗎？', cls: '哲學概論・週二班', due: '11 月 7 日已結束', status: 'done', state: 4, rounds: 9, cta: '看學習總結', plate: 3 },
  ];
  const CLASSES = [
    { name: '哲學概論・週二班', who: '林老師', count: '32 位同學', note: '週二 第 3–4 節' },
    { name: '哲學概論・讀書會', who: '陳助教', count: '12 位同學', note: '自由參加，不計分' },
  ];
  const T_ACTS = [
    { tav: 'XIII', title: '說謊永遠是錯的嗎？', status: 'draft', s: '草稿', part: '—', due: '11 月 21 日' },
    { tav: 'XII', title: '民主一定比較公平嗎？', status: 'live', s: '進行中', part: '21 / 32', due: '11 月 14 日' },
    { tav: 'XI', title: '知識就是力量嗎？', status: 'done', s: '已結束', part: '30 / 32', due: '11 月 7 日' },
    { tav: 'X', title: '我們能確定自己不是在做夢嗎？', status: 'done', s: '已結束', part: '29 / 32', due: '10 月 31 日' },
  ];
  const CLAIMS = [
    { claim: '有保障少數的民主比較公平', n: 11 },
    { claim: '民主程序公平，但結果不一定公平', n: 7 },
    { claim: '公平要看是否照顧到最弱勢的人', n: 2 },
  ];
  const ROSTER = [
    ['王小明', 'student01@demo.tw', '已加入'], ['李宜庭', 'student02@demo.tw', '已加入'], ['張家豪', 'student03@demo.tw', '已加入'],
    ['陳思妤', 'student04@demo.tw', '尚未登入'], ['黃柏翰', 'student05@demo.tw', '已加入'],
  ];
  const TALK = [
    ['me', '民主一定比較公平，因為每個人都有一票。'],
    ['ai', '你說的「公平」，指的是每個人都有一票的程序，還是結果對每個人都好？'],
    ['me', '我覺得是程序吧，大家都有機會表達意見。'],
    ['ai', '如果多數人投票決定拿走少數人的權利，程序一樣是一人一票。這個結果你還會說公平嗎？'],
    ['me', '那就不公平了，所以要加上保障少數的規則。'],
    ['ai', '你剛才把主張改成「有保障少數的民主比較公平」。這個保障是民主本身的一部分，還是民主之外另外加上的？'],
  ];
  const FOLLOWUPS = [
    '如果保障少數的規則也要經過多數同意才能訂定，它還能防止多數壓迫少數嗎？',
    '你能舉一個具體的例子，說明這種保障在現實裡怎麼運作嗎？',
    '到目前為止，你的主張是什麼？用一句話說說看，並說出你最重要的理由。',
  ];

  const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));
  const $ = (s, r = document) => r.querySelector(s);

  let cfg = {};
  let role = 'student';
  let route = 'home';
  let talk = TALK.map(t => [...t]);
  let rounds = 5;

  const roleOf = r => (r.startsWith('t-') ? 'teacher' : 'student');
  const navItem = id => [...NAV.student, ...NAV.teacher].find(n => n.id === id);

  /* ---------- pages ---------- */
  function studentHome() {
    const rows = ACTIVITIES.map(a => `
      <li class="plate-row" data-plate="${a.plate}">
        <span class="tav">TAV. ${a.tav}</span>
        <div>
          <h3>${esc(a.title)}</h3>
          <p class="meta"><span>${esc(a.cls)}</span><span>${esc(a.due)}</span>${cfg.rowMeta ? cfg.rowMeta(a) : ''}</p>
        </div>
        <div class="act">${a.status === 'done'
          ? `<button class="link" type="button" data-go="talk">${a.cta}</button>`
          : `<button class="btn${a.status === 'new' ? ' quiet' : ''}" type="button" data-go="talk">${a.cta}</button>`}</div>
      </li>`).join('');
    const cls = CLASSES.map(c => `<div class="class-item"><b>${esc(c.name)}</b><span class="note">${esc(c.who)}・${esc(c.count)}・${esc(c.note)}</span></div>`).join('');
    return `
      ${cfg.pageHead ? cfg.pageHead('home') : `<header><h1 class="page-title">我的教室</h1><p class="page-sub">本週有 2 個討論活動等你，題目由老師指定。</p></header>`}
      <section aria-labelledby="h-acts">
        <div class="section-h"><h2 id="h-acts">討論活動</h2><span class="note">依截止日排序</span></div>
        <ul class="plates">${rows}</ul>
      </section>
      <section aria-labelledby="h-cls">
        <div class="section-h"><h2 id="h-cls">我加入的教室</h2><span class="note">名單由老師匯入</span></div>
        <div class="class-list">${cls}</div>
      </section>`;
  }

  function teacherClass(tab = 'acts') {
    const acts = `
      <div class="table-wrap"><table class="table">
        <thead><tr><th scope="col">版號</th><th scope="col">題目</th><th scope="col">狀態</th><th scope="col" class="num">參與</th><th scope="col">截止</th><th scope="col"><span class="sr">操作</span></th></tr></thead>
        <tbody>${T_ACTS.map(a => `<tr><td class="latin">TAV. ${a.tav}</td><td><b>${esc(a.title)}</b></td><td><span class="status ${a.status}">${a.s}</span></td><td class="num">${a.part}</td><td>${a.due}</td><td><button class="link" type="button">${a.status === 'draft' ? '編輯' : '查看'}</button></td></tr>`).join('')}</tbody>
      </table></div>`;
    const max = Math.max(...CLAIMS.map(c => c.n));
    const claims = `
      <p class="note">TAV. XII「民主一定比較公平嗎？」・21 位同學的最後主張（AI 歸納，教師可調整分組）</p>
      <ol class="claims">${CLAIMS.map(c => `<li><span class="c">${esc(c.claim)}</span><span class="bar" style="--w:${(c.n / max) * 100}%"><i></i></span><b>${c.n}</b></li>`).join('')}</ol>`;
    const roster = `
      <div class="table-wrap"><table class="table">
        <thead><tr><th scope="col">姓名</th><th scope="col">電子郵件</th><th scope="col">狀態</th></tr></thead>
        <tbody>${ROSTER.map(r => `<tr><td>${r[0]}</td><td>${r[1]}</td><td>${r[2] === '已加入' ? '<span class="status live">已加入</span>' : '<span class="status">尚未登入</span>'}</td></tr>`).join('')}</tbody>
      </table></div>`;
    const body = { acts, claims, roster }[tab];
    const crumbs = `<nav class="crumbs" aria-label="麵包屑"><a href="#t-classes" data-go="t-classes">教室管理</a><span aria-hidden="true">/</span><span>哲學概論・週二班</span></nav>`;
    return `
      ${cfg.pageHead ? cfg.pageHead('t-classes', crumbs) : `${crumbs}<header><h1 class="page-title">哲學概論・週二班</h1><p class="page-sub">週二 第 3–4 節・32 位同學</p></header>`}
      <div style="display:flex;justify-content:space-between;gap:16px;align-items:flex-end;flex-wrap:wrap">
        <div class="tabs" role="tablist" aria-label="教室內容">
          <button class="tab" role="tab" data-tab="acts" aria-selected="${tab === 'acts'}">討論活動</button>
          <button class="tab" role="tab" data-tab="claims" aria-selected="${tab === 'claims'}">論點分布</button>
          <button class="tab" role="tab" data-tab="roster" aria-selected="${tab === 'roster'}">學生名單</button>
        </div>
        <button class="btn" type="button">${icon('plus')}新增討論活動</button>
      </div>
      <section role="tabpanel">${body}</section>`;
  }

  function stub(id) {
    const n = navItem(id);
    return `
      ${cfg.pageHead ? cfg.pageHead(id) : `<header><h1 class="page-title">${n.zh}</h1></header>`}
      <p class="alert info">示意原型只畫了「我的教室」、「對話」與教師的「教室詳細」。這一區的內容請見 Figma 對應頁面；這裡只用來看導覽怎麼帶你到這棟建築。</p>`;
  }

  /* ---------- conversation ---------- */
  function renderTalk() {
    const col = $('#talk-col');
    const last = talk.length - 1;
    col.innerHTML = talk.map((t, i) => {
      const [who, text] = t;
      if (who === 'thinking') return `<div class="turn ai thinking" aria-live="polite"><span class="who">蘇格拉底　INTER.</span><span>正在想下一個問題<span class="dots"></span></span></div>`;
      const cur = who === 'ai' && i === last ? ' current' : '';
      return `<div class="turn ${who}${cur}"><span class="who">${who === 'ai' ? '蘇格拉底　INTER.' : 'DISC.　你'}</span><span>${esc(text)}</span></div>`;
    }).join('');
    if (cfg.onTalk) cfg.onTalk(rounds, talk);
  }

  function send(text) {
    text = text.trim();
    if (!text) return;
    talk.push(['me', text]);
    talk.push(['thinking', '']);
    rounds += 1;
    renderTalk();
    scrollTalk();
    setTimeout(() => {
      talk.pop();
      talk.push(['ai', FOLLOWUPS[(rounds - 6) % FOLLOWUPS.length]]);
      renderTalk();
      scrollTalk();
    }, 1400);
  }
  const scrollTalk = () => window.scrollTo({ top: document.body.scrollHeight, behavior: matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth' });

  /* ---------- shell ---------- */
  function renderTopbarAccount() {
    const p = PEOPLE[role];
    $('#account').innerHTML = `
      <button type="button" aria-haspopup="true" aria-expanded="false" id="acct-btn"><span class="avatar" aria-hidden="true">${p.initial}</span><span class="name">${p.name}</span>${icon('chevron', 'ico sm')}</button>
      <div class="menu" id="acct-menu" hidden>
        <div class="who"><b>${p.name}</b><span class="note">${p.role}・${p.sub}</span></div>
        <div class="row"><label for="lang">語言</label><select id="lang"><option>繁體中文</option><option>English</option></select></div>
        <button type="button" id="logout">登出</button>
      </div>`;
    const pts = $('#points');
    if (pts) { pts.hidden = p.points == null; if (p.points != null) pts.innerHTML = `<span>點數</span><b>${p.points}</b>`; }
  }

  function go(r, opts = {}) {
    route = r;
    const nextRole = r === 'talk' ? 'student' : roleOf(r);
    role = nextRole;
    const focus = r === 'talk';
    $('#work').hidden = focus;
    $('#focus').hidden = !focus;
    closeDrawer();
    if (focus) {
      renderTalk();
      if (cfg.onFocus) cfg.onFocus();
    } else {
      if ($('#side').dataset.role !== role) renderTopbarAccount();
      $('#side').dataset.role = role;
      $('#side').innerHTML = cfg.side(role, r, NAV[role]);
      const page = $('#page');
      page.innerHTML = r === 'home' ? studentHome() : r === 't-classes' ? teacherClass(opts.tab) : stub(r);
      if (cfg.onRoute) cfg.onRoute(r, role, navItem(r), opts);
    }
    document.querySelectorAll('.proto-bar [data-go]').forEach(b => b.setAttribute('aria-pressed', String(b.dataset.go === r || (b.dataset.go === 't-classes' && role === 'teacher' && !focus) || (b.dataset.go === 'home' && role === 'student' && !focus && r !== 'talk'))));
    if (!opts.keepScroll) window.scrollTo(0, 0);
    if (location.hash.slice(1) !== r) history.replaceState(null, '', '#' + r);
  }

  function openDrawer() { $('#side').classList.add('open'); const s = $('#scrim'); s.hidden = false; requestAnimationFrame(() => s.classList.add('open')); $('#menu-btn').setAttribute('aria-expanded', 'true'); $('#side').querySelector('a,button')?.focus(); }
  function closeDrawer() { const side = $('#side'); if (!side) return; side.classList.remove('open'); const s = $('#scrim'); s.classList.remove('open'); setTimeout(() => { if (!s.classList.contains('open')) s.hidden = true; }, 220); $('#menu-btn')?.setAttribute('aria-expanded', 'false'); }

  function wire() {
    document.addEventListener('click', e => {
      const g = e.target.closest('[data-go]');
      if (g) { e.preventDefault(); go(g.dataset.go); return; }
      const t = e.target.closest('[data-tab]');
      if (t) { go('t-classes', { tab: t.dataset.tab, keepScroll: true }); $(`[data-tab="${t.dataset.tab}"]`)?.focus(); return; }
      if (e.target.closest('#logout')) { location.href = '../b-roman-forum/vedute.html'; return; }
      if (e.target.closest('#menu-btn')) { openDrawer(); return; }
      if (e.target.closest('#scrim')) { closeDrawer(); return; }
      const ab = e.target.closest('#acct-btn');
      const menu = $('#acct-menu');
      if (ab) { menu.hidden = !menu.hidden; ab.setAttribute('aria-expanded', String(!menu.hidden)); return; }
      if (menu && !menu.hidden && !e.target.closest('#acct-menu')) { menu.hidden = true; $('#acct-btn').setAttribute('aria-expanded', 'false'); }
    });
    document.addEventListener('keydown', e => {
      if (e.key !== 'Escape') return;
      closeDrawer();
      const menu = $('#acct-menu'); if (menu && !menu.hidden) { menu.hidden = true; $('#acct-btn').focus(); }
    });
    const ta = $('#composer-input');
    const submit = () => { send(ta.value); ta.value = ''; ta.style.height = ''; };
    $('#composer').addEventListener('submit', e => { e.preventDefault(); submit(); });
    ta.addEventListener('keydown', e => { if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); submit(); } });
    ta.addEventListener('input', () => { ta.style.height = 'auto'; ta.style.height = Math.min(ta.scrollHeight, 160) + 'px'; });
    window.addEventListener('hashchange', () => { const h = location.hash.slice(1); if (h && h !== route) go(h); });
  }

  window.Interior = {
    icon, NAV, ACTIVITIES, esc,
    get rounds() { return rounds; },
    go,
    init(c) {
      cfg = c;
      wire();
      const h = location.hash.slice(1);
      const valid = h === 'talk' || h === 'home' || [...NAV.student, ...NAV.teacher].some(n => n.id === h);
      go(valid ? h : 'home');
    },
  };
})();
