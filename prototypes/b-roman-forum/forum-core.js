// Shared core for the prototype-B art-direction proposals (Moneta, Vedute, Salve).
// Each proposal supplies its own materials, ground, renderer pass and UI skin;
// this file owns the forum's geometry, the four camera stations and the login flow.
import * as THREE from 'three';

export { THREE };

export const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)');
export const isPhone = () => innerWidth < 760;
export const $ = (s, r = document) => r.querySelector(s);

/* ---------------- the forum, Roman buildings only ----------------
   Ground plane y = 0. Looking west from the Via Sacra: Tabularium closes the far end,
   the Temple of Saturn and the Rostra stand before it, the Curia on the north side,
   the Basilica Julia along the south, the round Temple of Vesta near the viewer. */
export const SITES = {
  tabularium: [0, -7.2], saturn: [3.6, -4.6], rostra: [-0.6, -2.6], curia: [-6.2, -1.6],
  basilica: [7.2, 0.4], castor: [5.2, 4.4], vesta: [1.8, 6.4],
};

export function buildForum({ mat, onMesh = () => {}, ruined = false, detail = 1 }) {
  const root = new THREE.Group();
  const sites = {};
  const M = (geo, role, x = 0, y = 0, z = 0, parent) => {
    const m = new THREE.Mesh(geo, mat(role));
    m.position.set(x, y, z);
    m.userData.role = role;
    onMesh(m, role);
    (parent || root).add(m);
    return m;
  };
  const box = (w, h, d) => new THREE.BoxGeometry(w, h, d);
  const seg = Math.max(8, Math.round(12 * detail));
  const col = (r, h) => new THREE.CylinderGeometry(r * 0.88, r, h, seg);
  const site = (name, ry = 0) => { const g = new THREE.Group(); g.position.set(SITES[name][0], 0, SITES[name][1]); g.rotation.y = ry; root.add(g); sites[name] = g; return g; };
  const prism = (base, height, depth) => {
    const s = new THREE.Shape(); s.moveTo(-base / 2, 0); s.lineTo(base / 2, 0); s.lineTo(0, height); s.closePath();
    const g = new THREE.ExtrudeGeometry(s, { depth, bevelEnabled: false }); g.translate(0, 0, -depth / 2); return g;
  };
  // a row of round arches cut into a wall: piers + lintel blocks, so every material reads them
  const arcade = (g, n, bay, h, d, y0, role, z = 0) => {
    const w = n * bay;
    for (let i = 0; i <= n; i++) M(box(bay * 0.28, h, d), role, -w / 2 + i * bay, y0 + h / 2, z, g);
    M(box(w + bay * 0.28, h * 0.32, d), role, 0, y0 + h * 1.16, z, g);
    for (let i = 0; i < n; i++) {
      const a = new THREE.Mesh(new THREE.TorusGeometry(bay * 0.36, bay * 0.07, 6, seg, Math.PI), mat(role));
      a.position.set(-w / 2 + (i + 0.5) * bay, y0 + h * 0.86, z + d / 2); onMesh(a, role); g.add(a);
    }
  };

  // Tabularium: the records office, a high substructure wall with an arcade on top
  const tab = site('tabularium');
  M(box(14, 3.2, 2.2), 'stone2', 0, 1.6, 0, tab);
  for (let i = 0; i < 7; i++) M(box(0.7, 1.1, 0.06), 'shadow', -5.4 + i * 1.8, 1.4, 1.13, tab);
  arcade(tab, 7, 1.9, 1.7, 1.6, 3.2, 'stone', 0.3);
  M(box(14.2, 0.35, 2.4), 'stone', 0, 5.65, 0, tab);

  // Temple of Saturn: tall podium, eight-column front (ruined: the famous eight survivors only)
  const sat = site('saturn');
  M(box(4.6, 1.6, 6), 'stone2', 0, 0.8, 0, sat);
  M(box(2.4, 0.8, 1.4), 'stone2', 0, 0.4, 3.6, sat);
  for (let i = 0; i < 6; i++) M(col(0.17, 3.0), 'stone', -1.9 + i * 0.76, 3.1, 2.4, sat);
  for (const x of [-1.9, 1.9]) M(col(0.17, 3.0), 'stone', x, 3.1, 1.5, sat);
  M(box(4.6, 0.5, 1.9), 'stone', 0, 4.85, 1.95, sat);
  if (!ruined) {
    M(box(3.4, 3.0, 3.6), 'stone', 0, 3.1, -1.1, sat);
    M(box(4.6, 0.5, 6), 'stone', 0, 4.85, 0, sat);
    const p = M(prism(4.7, 1.0, 6.1), ['stone', 'roof'], 0, 5.1, 0, sat);
    p.userData.role = 'pediment';
  } else {
    M(prism(4.7, 1.0, 0.5), 'stone', 0, 5.1, 2.1, sat);
  }

  // Rostra: the speakers' platform, its face set with ship beaks (rostra)
  const ros = site('rostra');
  M(box(5.2, 1.6, 2.4), 'stone2', 0, 0.8, 0, ros);
  M(box(5.3, 0.22, 2.5), 'stone', 0, 1.7, 0, ros);
  for (let i = 0; i < 5; i++) {
    const beak = M(new THREE.ConeGeometry(0.16, 0.7, 6), 'bronze', -2 + i, 1.0, 1.42, ros);
    beak.rotation.x = Math.PI / 2;
  }
  M(box(1.6, 0.8, 0.8), 'stone2', 0, 0.4, -1.6, ros);
  const speakerA = new THREE.Group(), speakerB = new THREE.Group();
  for (const [g, role, x] of [[speakerA, 'figureA', -0.9], [speakerB, 'figureB', 0.9]]) {
    M(new THREE.CylinderGeometry(0.13, 0.24, 0.8, seg), role, 0, 0.4, 0, g);
    M(new THREE.SphereGeometry(0.16, seg, 8), role, 0, 0.98, 0, g);
    g.position.set(x, 1.81, 0.2); ros.add(g);
  }

  // Curia Julia: tall brick hall, gable, bronze double door facing the square (+x)
  const cur = site('curia', Math.PI / 2);
  M(box(4.2, 4.6, 3.6), 'brick', 0, 2.3, 0, cur);
  M(prism(4.4, 1.1, 3.8), ['stone', 'roof'], 0, 4.6, 0, cur);
  M(box(4.3, 0.3, 3.7), 'stone', 0, 4.5, 0, cur);
  M(box(2.0, 0.4, 1.0), 'stone2', 0, 0.2, 2.2, cur);
  M(box(1.5, 2.3, 0.1), 'shadow', 0, 1.55, 1.81, cur);
  for (const x of [-1.2, 0, 1.2]) M(box(0.6, 1.1, 0.06), 'shadow', x, 3.5, 1.81, cur);
  const doorL = new THREE.Group(), doorR = new THREE.Group();
  doorL.position.set(-0.7, 1.55, 1.86); doorR.position.set(0.7, 1.55, 1.86);
  M(box(0.68, 2.2, 0.08), 'bronze', 0.34, 0, 0, doorL);
  M(box(0.68, 2.2, 0.08), 'bronze', -0.34, 0, 0, doorR);
  cur.add(doorL, doorR);

  // Basilica Julia: long two-storey arcade on the south side
  const bas = site('basilica', -Math.PI / 2);
  M(box(11, 0.4, 3), 'stone2', 0, 0.2, 0, bas);
  arcade(bas, 6, 1.75, 1.6, 0.5, 0.4, 'stone', 1.2);
  if (!ruined) arcade(bas, 6, 1.75, 1.3, 0.5, 2.5, 'stone', 1.2);

  // Temple of Castor: three tall columns stand (ruined) or the whole front (intact)
  const cas = site('castor', -Math.PI / 2.4);
  M(box(3.6, 1.8, 4.4), 'stone2', 0, 0.9, 0, cas);
  const nCas = ruined ? 3 : 6;
  for (let i = 0; i < nCas; i++) M(col(0.16, 3.4), 'stone', -1.4 + i * 0.56, 3.5, 1.7, cas);
  M(box(ruined ? 1.6 : 3.4, 0.45, 0.6), 'stone', ruined ? -0.84 : 0, 5.4, 1.7, cas);

  // Temple of Vesta: the round one
  const ves = site('vesta');
  M(new THREE.CylinderGeometry(1.5, 1.6, 0.8, seg * 2), 'stone2', 0, 0.4, 0, ves);
  M(new THREE.CylinderGeometry(1.0, 1.0, 2.2, seg * 2), 'stone', 0, 1.9, 0, ves);
  for (let i = 0; i < 12; i++) { const a = (i / 12) * Math.PI * 2; M(col(0.1, 2.3), 'stone', Math.cos(a) * 1.35, 1.95, Math.sin(a) * 1.35, ves); }
  if (!ruined) { const roof = M(new THREE.ConeGeometry(1.6, 0.9, seg * 2), 'roof', 0, 3.55, 0, ves); roof.userData.role = 'roof'; }

  const anchors = {
    2: new THREE.Vector3(SITES.rostra[0], 4.2, SITES.rostra[1]),
    3: new THREE.Vector3(SITES.tabularium[0], 6.6, SITES.tabularium[1]),
    4: new THREE.Vector3(SITES.curia[0], 6.4, SITES.curia[1]),
  };
  return { root, sites, anchors, doorL, doorR, speakerA, speakerB };
}

/* The idea above the Rostra: a rough body with one morph target that each proposal
   defines (a struck coin, a clean engraving, a finished emblema). */
export function makeIdea(material, { radius = 0.72, target = (v, r) => v.clone().multiplyScalar(r), detail = 2 } = {}) {
  const geo = new THREE.IcosahedronGeometry(radius, detail);
  const pos = geo.attributes.position;
  const goal = new Float32Array(pos.array.length);
  const v = new THREE.Vector3();
  for (let i = 0; i < pos.count; i++) {
    v.fromBufferAttribute(pos, i).normalize();
    const t = target(v, radius);
    goal.set([t.x, t.y, t.z], i * 3);
    const n = 1 + 0.34 * Math.sin(v.x * 5.1 + 1.3) * Math.cos(v.y * 4.3) + 0.24 * Math.sin(v.z * 6.7 + v.x * 2.0);
    pos.setXYZ(i, v.x * radius * n, v.y * radius * n, v.z * radius * n);
  }
  geo.morphAttributes.position = [new THREE.Float32BufferAttribute(goal, 3)];
  geo.computeVertexNormals();
  const mesh = new THREE.Mesh(geo, material);
  mesh.morphTargetInfluences = [0];
  return mesh;
}

/* ---------------- camera stations ---------------- */
export function makeRig(camera, stations, { panelShift = 230, phonePush = 0.2 } = {}) {
  const cam = { px: 0, py: 0, pz: 0, tx: 0, ty: 0, tz: 0 };
  const view = (n) => {
    const s = stations[n];
    const k = isPhone() ? (s.phoneK ?? 1.3) : 1;
    const p = s.pos.map((c, i) => s.tgt[i] + (c - s.tgt[i]) * k);
    return { px: p[0], py: p[1], pz: p[2], tx: s.tgt[0], ty: s.tgt[1], tz: s.tgt[2] };
  };
  const apply = () => { camera.position.set(cam.px, cam.py, cam.pz); camera.lookAt(cam.tx, cam.ty, cam.tz); };
  const resize = (renderer) => {
    const w = innerWidth, h = innerHeight;
    renderer.setPixelRatio(Math.min(devicePixelRatio, isPhone() ? 1.5 : 2));
    renderer.setSize(w, h, false);
    camera.aspect = w / h;
    camera.fov = w < h ? 48 : 34;
    // keep the subject clear of the panel by shifting the projection, never the camera
    if (isPhone()) camera.setViewOffset(w, h, 0, h * phonePush, w, h);
    else camera.setViewOffset(w, h, -Math.min(panelShift, w * 0.17), 0, w, h);
    camera.updateProjectionMatrix();
  };
  return { cam, view, apply, resize };
}

/* ---------------- render loop with lifecycle ---------------- */
export function runLoop(renderer, frame) {
  const clock = new THREE.Clock();
  const tick = () => frame(clock.getElapsedTime(), clock);
  const gsap = window.gsap;
  // stop drawing and pause every tween while the tab is hidden
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) { renderer.setAnimationLoop(null); gsap?.globalTimeline.pause(); clock.stop(); }
    else { clock.start(); gsap?.globalTimeline.resume(); renderer.setAnimationLoop(tick); }
  });
  renderer.setAnimationLoop(tick);
  return clock;
}

/* ---------------- pins projected from 3D anchors ---------------- */
export function makePins(container, anchors, labels, build) {
  const els = {};
  for (const [n, label] of Object.entries(labels)) {
    const b = document.createElement('button');
    b.type = 'button'; b.className = 'pin'; b.dataset.go = n; b.tabIndex = -1;
    b.setAttribute('aria-label', label.aria);
    build(b, label);
    container.append(b); els[n] = b;
  }
  const v = new THREE.Vector3();
  return (camera, current, hidden) => {
    for (const [n, el] of Object.entries(els)) {
      if (hidden || Number(n) === current) { el.hidden = true; continue; }
      v.copy(anchors[n]).project(camera);
      if (v.z > 1 || Math.abs(v.x) > 1.1 || Math.abs(v.y) > 1.1) { el.hidden = true; continue; }
      el.hidden = false;
      el.style.transform = `translate(${((v.x + 1) / 2) * innerWidth}px, ${((1 - v.y) / 2) * innerHeight}px) translate(-50%, -100%)`;
    }
  };
}

/* ---------------- accounts, homes, and the login flow ---------------- */
export const ACCOUNTS = [
  { email: 'student@demo.tw', role: 'student', label: '學生' },
  { email: 'teacher@demo.tw', role: 'teacher', label: '教師' },
  { email: 'admin@demo.tw', role: 'admin', label: '管理者' },
  { email: 'outsider@demo.tw', role: null, label: '名單外' },
];

export const HOMES = {
  student: { role: '學生', title: '我的教室', html: `
    <p class="home-note">活動中的對話與總結，教師會在活動結束後看到（唯讀）。從題目庫開始的個人練習只有你看得到。</p>
    <section class="home-card"><h2>[教室名稱 A]</h2><p class="home-meta">[教師姓名]</p>
      <div class="home-row"><span>[活動名稱 1]</span><span class="home-actions"><span class="home-tag">進行中</span><button type="button" class="ui-btn">繼續對話</button></span></div>
      <div class="home-row"><span>[活動名稱 2]</span><span class="home-actions"><span class="home-tag">已結束</span><button type="button" class="ui-btn quiet">查看總結</button></span></div></section>
    <section class="home-card"><h2>個人練習</h2><p class="home-meta">從題目庫挑一題自由練習，不屬於任何活動，教師看不到。</p><div class="home-actions"><button type="button" class="ui-btn quiet">瀏覽題目庫</button></div></section>` },
  teacher: { role: '教師', title: '我的教室', html: `
    <p class="home-note">你可以查看學生的學習總結，以及已結束活動的對話原文（唯讀）。進行中的對話與個人練習看不到。</p>
    <section class="home-card"><h2>[教室名稱 A]</h2><p class="home-meta">[N] 位學生，[N] 個活動</p>
      <div class="home-row"><span>[活動名稱 1]</span><span class="home-actions"><span class="home-tag">已結束</span><button type="button" class="ui-btn">查看學習總結</button></span></div>
      <div class="home-row"><span>[活動名稱 2]</span><span class="home-actions"><span class="home-tag">進行中</span><button type="button" class="ui-btn quiet">查看進度</button></span></div></section>
    <div class="home-actions"><button type="button" class="ui-btn">建立教室</button><button type="button" class="ui-btn quiet">匯入修課名單</button></div>` },
  admin: { role: '系統管理者', title: '所有教室', html: `
    <p class="home-note">介面中每則訊息一律顯示為 message；AI 總結、學生反思等敏感欄位遮蔽。</p>
    <section class="home-card"><h2>[教室名稱 A]</h2><p class="home-meta">[教師姓名]，名單 [N] 人，使用中</p></section>
    <section class="home-card"><h2>帳號與密碼</h2><p class="home-meta">不開放自行註冊；帳號由修課名單開通，密碼由管理者重設。</p><div class="home-actions"><button type="button" class="ui-btn quiet">重設密碼</button></div></section>` },
};

/* Wires the shared markup (see any proposal's index for the ids) to the proposal's hooks:
   hooks.station(n, prev)  hooks.enter(acc) -> Promise  hooks.leave()  hooks.homeShown() */
export function wireShell(hooks) {
  const state = { current: 1, busy: false, who: '' };
  const panels = [1, 2, 3, 4].map((n) => $('#st-' + n));
  const go = (n, { focus = true } = {}) => {
    n = Number(n);
    if (state.busy || !panels[n - 1]) return;
    const prev = state.current; state.current = n;
    panels.forEach((p, i) => p.classList.toggle('is-active', i + 1 === n));
    document.querySelectorAll('[data-go]').forEach((b) => {
      if (!b.classList.contains('station')) return;
      if (Number(b.dataset.go) === n) b.setAttribute('aria-current', 'step'); else b.removeAttribute('aria-current');
    });
    hooks.station(n, prev);
    if (focus) $('[data-h]', panels[n - 1])?.focus({ preventScroll: true });
  };
  document.addEventListener('click', (e) => { const b = e.target.closest('[data-go]'); if (b) go(b.dataset.go); });
  addEventListener('keydown', (e) => {
    if (e.target.closest('input, textarea') || !$('#home').hidden) return;
    if (e.key === 'ArrowRight') go(Math.min(4, state.current + 1));
    if (e.key === 'ArrowLeft') go(Math.max(1, state.current - 1));
  });

  const views = { login: $('#v-login'), google: $('#v-google'), blocked: $('#v-blocked') };
  const view = (name, focusSel) => { for (const [k, el] of Object.entries(views)) el.hidden = k !== name; if (focusSel) $(focusSel)?.focus({ preventScroll: true }); };
  const email = $('#email'), pw = $('#pw'), submit = $('#submit');
  const submitLabel = submit.textContent;
  const fieldError = (input, msg) => { const err = $('#' + input.id + '-err'); err.textContent = msg; err.hidden = !msg; input.setAttribute('aria-invalid', msg ? 'true' : 'false'); };
  const clearBanners = () => { $('#a-bad').hidden = true; $('#a-timeout').hidden = true; };
  const resetSubmit = () => { submit.disabled = false; submit.textContent = submitLabel; };

  async function route(em) {
    resetSubmit();
    const acc = ACCOUNTS.find((a) => a.email === em.toLowerCase());
    if (!acc) { $('#a-bad').hidden = false; pw.value = ''; pw.focus(); return; }
    if (!acc.role) { $('#blocked-who').textContent = acc.email; $('#a-recheck').hidden = true; view('blocked', '#h4b'); return; }
    if (state.current !== 4) go(4, { focus: false });
    state.busy = true;
    try { await hooks.enter(acc); } finally { state.busy = false; }
    showHome(acc);
  }
  $('#login').addEventListener('submit', (e) => {
    e.preventDefault();
    const em = email.value.trim();
    let bad = false;
    if (!em) { fieldError(email, '請輸入電子郵件。'); bad = true; }
    else if (!/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(em)) { fieldError(email, '電子郵件格式不正確，例如 name@school.edu。'); bad = true; }
    else fieldError(email, '');
    if (!pw.value) { fieldError(pw, '請輸入密碼。'); bad = true; } else fieldError(pw, '');
    if (bad) { (email.getAttribute('aria-invalid') === 'true' ? email : pw).focus(); return; }
    clearBanners();
    submit.disabled = true; submit.innerHTML = '<span class="spin" aria-hidden="true"></span>登入中';
    setTimeout(() => route(em), 700);
  });
  $('#google').addEventListener('click', () => view('google', '#h4g'));
  $('#g-cancel').addEventListener('click', () => view('login', '#h4d'));
  $('#recheck').addEventListener('click', () => { $('#a-recheck').hidden = false; });
  $('#other').addEventListener('click', () => { email.value = ''; pw.value = ''; view('login', '#email'); });
  for (const a of [ACCOUNTS[0], ACCOUNTS[3], ACCOUNTS[1]]) {
    const li = document.createElement('li');
    const b = document.createElement('button'); b.type = 'button';
    const av = document.createElement('span'); av.className = 'avatar'; av.setAttribute('aria-hidden', 'true'); av.textContent = a.email[0].toUpperCase();
    const tx = document.createElement('span'); tx.textContent = a.email;
    b.append(av, tx);
    b.addEventListener('click', () => { view('login'); email.value = a.email; route(a.email); });
    li.append(b); $('#accounts').append(li);
  }
  for (const a of ACCOUNTS) {
    const b = document.createElement('button'); b.type = 'button'; b.textContent = a.label;
    b.addEventListener('click', () => { email.value = a.email; pw.value = 'demo-pass'; fieldError(email, ''); fieldError(pw, ''); clearBanners(); submit.focus(); });
    $('#demo').append(b);
  }

  function showHome(acc) {
    state.who = acc.email;
    const h = HOMES[acc.role];
    $('#home-role').textContent = h.role;
    $('#home-h').textContent = h.title;
    $('#home-body').innerHTML = h.html;
    $('#home').hidden = false;
    hooks.homeShown?.();
    $('#home-h').focus({ preventScroll: true });
    pw.value = '';
  }
  function leaveHome(timeout) {
    $('#home').hidden = true;
    hooks.leave?.();
    view('login');
    if (timeout) { email.value = state.who; $('#a-timeout').hidden = false; go(4, { focus: false }); pw.focus(); }
    else { email.value = ''; go(1); }
  }
  $('#back').addEventListener('click', () => leaveHome(false));
  $('#sim-timeout').addEventListener('click', () => leaveHome(true));

  return { state, go };
}

/* Shared markup for the panels and the home page, so each proposal only skins it. */
export const TALK = [
  ['s', '民主一定比較公平。'], ['a', '「公平」指的是誰的標準？'],
  ['s', '多數人同意就是標準。'], ['a', '少數人被忽略時呢？'],
  ['s', '那要加上保障少數。'], ['a', '你剛才改變了什麼主張？'],
];
