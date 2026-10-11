'use strict';
/* nuubOS Web administration (EPIC-038): the console's control centre in a
 * browser. Same grammar as nuubUI Settings (rail, sectioned pages, tonal
 * rows, choice sheets, double-press confirmations) for mouse, keyboard
 * and touch; the active user's Mood and language.
 *
 * This file is the shell: services access, live updates, i18n, the row
 * toolkit, keyboard navigation and routing. pages.js holds the pages.
 * Nothing here decides product rules: every value comes from a service
 * reply (/api/get) and every change is a service command (/api/do). */

/* ------------------------------------------------------------ basics */

const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];

/* h('div', {class, text, on: {click}, vars: {'--x': v}, data: {}}, ...children) */
function h(tag, attrs, ...kids) {
  const el = document.createElement(tag);
  if (attrs) {
    for (const [k, v] of Object.entries(attrs)) {
      if (v === undefined || v === null || v === false) continue;
      if (k === 'class') el.className = v;
      else if (k === 'text') el.textContent = v;
      else if (k === 'on') for (const [e, f] of Object.entries(v)) el.addEventListener(e, f);
      else if (k === 'vars') for (const [n, x] of Object.entries(v)) el.style.setProperty(n, x);
      else if (k === 'data') for (const [n, x] of Object.entries(v)) el.dataset[n] = x;
      else if (k === 'props') Object.assign(el, v);
      else el.setAttribute(k, v === true ? '' : v);
    }
  }
  for (const k of kids.flat(Infinity)) {
    if (k === null || k === undefined || k === false) continue;
    el.append(k instanceof Node ? k : document.createTextNode(String(k)));
  }
  return el;
}

const icon = (name, cls = '') => h('i', { class: 'icon ' + cls, vars: { '--i': `url(icons/${name}.svg)` }, 'aria-hidden': 'true' });
const list = (x) => (Array.isArray(x) ? x : x === undefined || x === null || x === '' ? [] : [x]);
/* Rows of a key=value reply: every entry as a field list. */
const rows = (x) => list(x).map((r) => (Array.isArray(r) ? r : [r]));
const img = (path) => (path ? '/api/img?' + new URLSearchParams({ p: path }) : '');

function fmt(s, args) {
  return String(s).replace(/\{(\d)\}/g, (_, i) => (args[i] !== undefined ? args[i] : ''));
}

/* -------------------------------------------------------------- logo */

/* The nuubOS logo: the dot-matrix wordmark, "nuub" in the Mood's text
 * colour, "OS" in its accent (same dot table as nuubos-splash-render.h). */
const LOGO = [
  '                  #       ####   ####',
  '                  #      ##  ## #    ',
  '####  #   # #   # ####   #    # #    ',
  '#   # #   # #   # #   #  #    #  ### ',
  '#   # #   # #   # #   #  #    #     #',
  '#   # #   # #   # #   #  ##  ##     #',
  '#   #  ####  #### ####    ####  #### ',
];

function logoSvg(cls = '') {
  const NS = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(NS, 'svg');
  svg.setAttribute('viewBox', '0 0 37 7');
  svg.setAttribute('class', 'logo ' + cls);
  svg.setAttribute('role', 'img');
  svg.setAttribute('aria-label', 'nuubOS');
  LOGO.forEach((line, r) => [...line].forEach((ch, c) => {
    if (ch !== '#') return;
    const d = document.createElementNS(NS, 'rect');
    d.setAttribute('x', c); d.setAttribute('y', r);
    d.setAttribute('width', 0.84); d.setAttribute('height', 0.84); d.setAttribute('rx', 0.21);
    d.setAttribute('class', c >= 25 ? 'a' : 't');
    svg.append(d);
  }));
  return svg;
}

/* --------------------------------------------------------------- i18n */

/* Console strings (the nuubOS catalogs, same index as nuubUI) and the
 * strings only this site needs (i18n.js); both follow the language of the
 * console's active user. */
let CAT = [];
let LANG = 'en';
const C = (i, fallback, ...a) => fmt(CAT[i] || fallback, a);
const T = (k, ...a) => fmt((WEB[LANG] && WEB[LANG][k]) || WEB.en[k] || k, a);

function sizeText(bytes) {
  const n = Number(bytes) || 0;
  const u = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0, v = n;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return (i ? v.toFixed(v < 10 ? 1 : 0) : v) + ' ' + u[i];
}

function dateText(epoch, withTime = true) {
  const n = Number(epoch);
  if (!n) return C(302, 'Never');
  const d = new Date(n * 1000);
  return d.toLocaleDateString(LANG, { year: 'numeric', month: 'short', day: 'numeric' }) +
    (withTime ? ' ' + d.toLocaleTimeString(LANG, { hour: '2-digit', minute: '2-digit' }) : '');
}

/* "today", "yesterday", "N days ago" like the console. */
function agoText(epoch) {
  const n = Number(epoch);
  if (!n) return C(405, 'Never played');
  const day = (t) => Math.floor((t - new Date(t).getTimezoneOffset() * 60000) / 86400000);
  const days = day(Date.now()) - day(n * 1000);
  if (days <= 0) return C(402, 'today');
  if (days === 1) return C(403, 'yesterday');
  return C(404, '{0} days ago', days);
}

function playtimeText(sec) {
  const s = Number(sec) || 0;
  const hh = Math.floor(s / 3600), mm = Math.floor((s % 3600) / 60);
  return hh ? C(407, '{0} h {1} min', hh, String(mm).padStart(2, '0')) : C(408, '{0} min', mm);
}

const gamesText = (n) => (Number(n) === 1 ? C(401, '1 game') : C(400, '{0} games', n));

/* ---------------------------------------------------------- services */

class ApiError extends Error {
  constructor(code, status) { super(code); this.code = code; this.status = status; }
}

async function request(url, opts = {}) {
  let r;
  try {
    r = await fetch(url, { credentials: 'same-origin', ...opts });
  } catch (e) {
    throw new ApiError('network', 0);
  }
  if (r.status === 401 && !url.startsWith('/api/login')) {
    signedOut();
    throw new ApiError('session', 401);
  }
  const body = await r.json().catch(() => ({}));
  if (!r.ok) throw new ApiError(body.error || String(r.status), r.status);
  return body;
}

const store = new Map();
const inflight = new Map();

/* A topic of /api/get, from the cache unless fresh is asked. */
async function get(t, params = {}, fresh = false) {
  const key = t + '?' + new URLSearchParams(params);
  if (!fresh && store.has(key)) return store.get(key);
  if (inflight.has(key)) return inflight.get(key);
  const p = request('/api/get?' + new URLSearchParams({ t, ...params }))
    .then((v) => { store.set(key, v); return v; })
    .finally(() => inflight.delete(key));
  inflight.set(key, p);
  return p;
}

const cached = (t, params = {}) => store.get(t + '?' + new URLSearchParams(params));

function forget(t) {
  for (const k of [...store.keys()]) if (k.startsWith(t + '?')) store.delete(k);
}

/* A product command; failures become a notification unless quiet. */
async function act(op, args = {}, { quiet = false, topics = [] } = {}) {
  try {
    const r = await request('/api/do', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-NuubOS': '1' },
      body: JSON.stringify({ op, ...args }),
    });
    for (const t of topics) forget(t);
    if (topics.length) refresh();
    return r;
  } catch (e) {
    if (!quiet && e.code !== 'session') toast(C(507, 'The operation failed'), reason(e.code), true);
    throw e;
  }
}

function reason(code) {
  const known = { network: T('offline'), space: C(493, 'Not enough free space'), exists: T('exists'),
    'no-user': T('no_user'), busy: C(790, 'The metadata service is busy, try again later') };
  return known[code] || '';
}

/* PUT a file with progress; resolves to the reply. */
function upload(dir, file, opts = {}, onProgress = () => {}) {
  return new Promise((resolve, reject) => {
    const x = new XMLHttpRequest();
    const q = { dir, name: opts.name || file.name };
    if (opts.after) q.after = opts.after;
    if (opts.replace) q.replace = '1';
    if (opts.mkdirs) q.mkdirs = '1';
    x.open('PUT', '/api/upload?' + new URLSearchParams(q));
    x.setRequestHeader('X-NuubOS', '1');
    x.upload.onprogress = (e) => e.lengthComputable && onProgress(e.loaded / e.total);
    x.onload = () => {
      let body = {};
      try { body = JSON.parse(x.responseText); } catch (e) { /* empty */ }
      if (x.status === 200) resolve(body);
      else {
        if (x.status === 401) signedOut();
        reject(new ApiError(body.error || String(x.status), x.status));
      }
    };
    x.onerror = () => reject(new ApiError('network', 0));
    x.send(file);
  });
}

/* Several files, one notification card with the progress. */
async function uploadFiles(files, dirFor, opts = {}) {
  const items = [...files];
  if (!items.length) return 0;
  const card = toast(T('uploading', items[0].name), '', false, 0);
  const bar = h('progress', { max: 100, value: 0 });
  card.body.append(bar);
  let done = 0;
  for (const f of items) {
    card.title.textContent = T('uploading', f.name);
    card.detail.textContent = items.length > 1 ? T('n_of_m', done + 1, items.length) : sizeText(f.size);
    try {
      await upload(dirFor(f), f, opts, (p) => { bar.value = p * 100; });
      done++;
    } catch (e) {
      card.close();
      toast(T('upload_failed', f.name), reason(e.code), true);
      return done;
    }
  }
  card.close();
  toast(T('upload_done'), items.length > 1 ? T('n_files', done) : items[0].name);
  return done;
}

/* ------------------------------------------------------ live updates */

let events = null;
let eventsLost = null;
const watchers = new Set();

function startEvents() {
  if (events) return;
  events = new EventSource('/api/events');
  events.onopen = () => {
    if (eventsLost) { eventsLost.close(); eventsLost = null; refreshAll(); }
  };
  events.onmessage = (m) => {
    let ev;
    try { ev = JSON.parse(m.data); } catch (e) { return; }
    onEvent(ev);
  };
  events.onerror = () => {
    if (!eventsLost && session) {
      eventsLost = toast(T('offline'), T('reconnecting'), true, 0);
      /* A closed session also ends the stream: find out which it was. */
      setTimeout(() => request('/api/session').catch(() => {}), 1500);
    }
  };
}

function stopEvents() {
  if (events) events.close();
  events = null;
}

const FAILURES = {
  wifi: (op, msg) => C(255, 'Connect failed: {0}', msg),
  bluetooth: (op, msg) => C(246, 'Bluetooth operation failed: {0}', msg),
  controllers: () => C(507, 'The operation failed'),
};

function onEvent(ev) {
  const t = ev.t;
  if (!t) return;
  if (ev.failed !== undefined) {
    const f = FAILURES[t];
    if (f) toast(f(ev.failed, ev.message), '', true);
  }
  forget(t);
  if (t === 'localization') loadLanguage().then(() => { renderShell(); refresh(); });
  if (t === 'themes' || t === 'users') loadMood();
  if (t === 'status' || t === 'regional' || t === 'users') topBar();
  for (const w of watchers) w(ev);
  if (current.topics.has(t) || current.topics.has('*')) refresh();
}

/* ------------------------------------------------------------- mood */

const TOKENS = ['accent', 'accent-light', 'accent-muted', 'accent-text', 'focus-fill', 'text', 'text-secondary',
  'text-dim', 'text-faint', 'text-disabled', 'background', 'surface', 'border', 'border-soft', 'warning'];

function applyMood(m) {
  if (!m) return;
  for (const k of TOKENS) if (m[k]) document.documentElement.style.setProperty('--' + k, m[k]);
}

async function loadMood() {
  try { applyMood((await get('themes', {}, true)).tokens); } catch (e) { /* keeps the current look */ }
}

async function loadLanguage() {
  try {
    const r = await request('/api/i18n');
    CAT = r.strings || [];
    LANG = r.lang || 'en';
  } catch (e) { /* English fallbacks */ }
  document.documentElement.lang = LANG;
}

/* ----------------------------------------------------- notifications */

/* A notification card like the console's; ms 0 stays until closed. */
function toast(title, detail = '', bad = false, ms = 4000) {
  const titleEl = h('div', { class: 'tt', text: title });
  const detailEl = h('div', { class: 'td', text: detail });
  const body = h('div', { class: 'tb' }, titleEl, detail ? detailEl : null);
  const card = h('div', { class: 'toast' + (bad ? ' bad' : '') }, icon(bad ? 'triangle-exclamation' : 'circle-info'), body);
  $('#toasts').append(card);
  const close = () => card.remove();
  if (ms) setTimeout(close, ms);
  card.addEventListener('click', close);
  if (!detail) body.append(detailEl);
  return { close, title: titleEl, detail: detailEl, body };
}

/* --------------------------------------------------------- the rows */

/* The armed row (double-press confirmation, docs/dev/ui.md): the element
 * itself switches to the error roles and carries the prompt. */
let armedKey = '';

function disarm() {
  if (!armedKey) return;
  armedKey = '';
  refresh();
}

/*
 * row({ key, title, detail, value, icon, image, toggle, chevron, pill,
 *       slider: { value, min, max, step, text, set }, on, arm: { prompt },
 *       disabled, danger, busy })
 */
function row(o) {
  const actionable = !o.disabled && (o.on || o.toggle !== undefined && o.set);
  const armed = o.arm && armedKey === o.key;
  const tag = actionable && !o.slider ? 'button' : 'div';
  const el = h(tag, {
    class: 'row' + (actionable ? ' act' : '') + (o.disabled ? ' off' : '') + (armed ? ' armed' : '') + (o.danger ? ' danger' : ''),
    type: tag === 'button' ? 'button' : null,
    tabindex: tag === 'div' && o.slider ? null : null,
    data: { key: o.key || o.title || '' },
  });
  if (o.icon || o.image) {
    el.append(h('span', { class: 'ri' }, o.image ? h('img', { src: o.image, alt: '', loading: 'lazy' }) : icon(o.icon)));
  }
  const detail = armed ? o.arm.prompt || C(191, 'Press again to confirm') : o.detail;
  el.append(h('span', { class: 'rt' }, h('span', { class: 'title', text: o.title }), detail ? h('span', { class: 'detail', text: detail }) : null));
  if (o.slider) {
    const s = o.slider;
    const input = h('input', { type: 'range', min: s.min ?? 0, max: s.max ?? 100, step: s.step ?? 1, value: s.value, 'aria-label': o.title,
      data: { key: (o.key || o.title) + ':slider' }, disabled: o.disabled ? true : null });
    const out = h('span', { class: 'sv', text: s.text ? s.text(s.value) : s.value + '%' });
    const paint = () => {
      const f = ((input.value - input.min) / (input.max - input.min)) * 100;
      input.style.setProperty('--f', f + '%');
      out.textContent = s.text ? s.text(Number(input.value)) : input.value + '%';
    };
    let timer = null;
    input.addEventListener('input', () => {
      paint();
      clearTimeout(timer);
      timer = setTimeout(() => s.set(Number(input.value)), 120);
    });
    input.addEventListener('change', () => { clearTimeout(timer); s.set(Number(input.value)); });
    paint();
    el.append(h('span', { class: 'slider' }, input, out));
  }
  if (o.busy) el.append(icon('circle-notch', 'busy'));
  if (o.value !== undefined && o.value !== '') el.append(h('span', { class: 'value', text: o.value }));
  if (o.pill) el.append(h('span', { class: 'pill ' + (o.pill.cls || ''), text: o.pill.text }));
  if (o.toggle !== undefined) el.append(h('span', { class: 'switch' + (o.toggle ? ' on' : ''), role: 'switch', 'aria-checked': String(!!o.toggle) }));
  if (o.chevron) el.append(icon('chevron-down', 'chev'));
  if (actionable) {
    el.addEventListener('click', async (e) => {
      e.stopPropagation();
      if (o.arm && armedKey !== o.key) {
        armedKey = o.key;
        refresh();
        return;
      }
      armedKey = '';
      try {
        if (o.toggle !== undefined && o.set) await o.set(!o.toggle);
        else await o.on(el);
      } catch (err) { /* reported by act() */ }
      refresh();
    });
  }
  return el;
}

function section(label, scope) {
  return h('div', { class: 'section' }, h('span', { text: label }),
    scope ? h('span', { class: 'chip', text: scope === 'user' ? C(312, 'USER') : C(313, 'DEVICE') }) : null);
}

const group = (...kids) => h('div', { class: 'rows' }, ...kids);

function button(label, on, o = {}) {
  const armed = o.arm && armedKey === o.key;
  const b = h('button', { class: 'btn ' + (o.cls || '') + (armed ? ' armed' : ''), type: 'button', title: o.title || null,
    data: { key: o.key || label }, disabled: o.disabled ? true : null },
  o.icon ? icon(o.icon) : null, o.round ? null : armed ? o.arm.prompt || C(191, 'Press again to confirm') : label);
  if (o.round) b.setAttribute('aria-label', label);
  b.addEventListener('click', async (e) => {
    e.stopPropagation();
    if (o.arm && armedKey !== o.key) { armedKey = o.key; refresh(); return; }
    armedKey = '';
    try { await on(b); } catch (err) { /* reported */ }
    refresh();
  });
  return b;
}

function empty(title, detail) {
  return h('div', { class: 'empty' }, h('b', { text: title }), detail ? h('div', { text: detail }) : null);
}

/* ------------------------------------------------- sheets and dialogs */

let modalClose = null;

function openModal(node, onClose) {
  const scrim = $('#scrim');
  scrim.replaceChildren(node);
  scrim.hidden = false;
  const prev = document.activeElement;
  modalClose = (v) => {
    scrim.hidden = true;
    scrim.replaceChildren();
    modalClose = null;
    if (prev && prev.isConnected) prev.focus({ preventScroll: true });
    onClose(v);
  };
  scrim.onclick = (e) => { if (e.target === scrim) modalClose(null); };
}

/* The choice sheet: options [{ value, label, detail }], resolves to the
 * value or null. Long lists get a filter field. */
function choose(title, options, currentValue) {
  return new Promise((resolve) => {
    const opts = h('div', { class: 'opts' });
    const filter = options.length > 12 ? h('input', { class: 'input filter', type: 'search', placeholder: T('filter'), 'aria-label': T('filter') }) : null;
    const fill = () => {
      const q = filter ? filter.value.trim().toLowerCase() : '';
      opts.replaceChildren(...options
        .filter((o) => !q || o.label.toLowerCase().includes(q) || String(o.value).toLowerCase().includes(q))
        .map((o) => {
          const sel = String(o.value) === String(currentValue);
          const b = h('button', { class: 'row act' + (sel ? ' sel' : ''), type: 'button', data: { key: 'opt:' + o.value } },
            h('span', { class: 'rt' }, h('span', { class: 'title', text: o.label }), o.detail ? h('span', { class: 'detail', text: o.detail }) : null),
            sel ? icon('check', 'check') : null);
          b.addEventListener('click', () => modalClose(o.value));
          return b;
        }));
    };
    fill();
    if (filter) filter.addEventListener('input', fill);
    openModal(h('div', { class: 'sheet', role: 'dialog', 'aria-label': title }, h('h3', { text: title }), filter, opts), resolve);
    const sel = $('.row.sel', opts) || $('.row', opts);
    (filter || sel)?.focus();
    if (sel) sel.scrollIntoView({ block: 'center' });
  });
}

/* A form dialog: fields [{ name, label, type, value }]; resolves to
 * { name: value } or null. */
function form(title, fields, { text = '', ok = C(82, 'Confirm') } = {}) {
  return new Promise((resolve) => {
    const inputs = {};
    const body = fields.map((f) => {
      const id = 'f-' + f.name;
      const input = f.options
        ? h('select', { class: 'input', id }, f.options.map((o) => h('option', { value: o.value, selected: String(o.value) === String(f.value) ? true : null, text: o.label })))
        : h('input', { class: 'input', id, type: f.type || 'text', value: f.value || '', autocomplete: f.type === 'password' ? 'off' : 'off',
          spellcheck: 'false', autocapitalize: 'off', placeholder: f.placeholder || '' });
      inputs[f.name] = input;
      return h('div', { class: 'field' }, h('label', { for: id, text: f.label }), input);
    });
    const err = h('div', { class: 'err' });
    const submit = (e) => {
      e.preventDefault();
      const v = {};
      for (const [k, el] of Object.entries(inputs)) v[k] = el.value;
      modalClose(v);
    };
    const f = h('form', { class: 'dialog', role: 'dialog', on: { submit } },
      h('h3', { text: title }), text ? h('p', { text }) : null, body, err,
      h('div', { class: 'actions' },
        h('button', { class: 'btn', type: 'button', text: C(619, 'Cancel'), on: { click: () => modalClose(null) } }),
        h('button', { class: 'btn primary', type: 'submit', text: ok })));
    openModal(f, resolve);
    Object.values(inputs)[0]?.focus();
  });
}

/* -------------------------------------------------- keyboard and focus */

const FOCUSABLE = 'button:not([disabled]), input:not([disabled]), select, a[href], [tabindex="0"]';

function focusables(root) {
  return $$(FOCUSABLE, root).filter((e) => e.offsetParent !== null && !e.closest('[hidden]'));
}

/* Spatial navigation (the console's D-pad): nearest element in the
 * direction, rows and grids alike. */
function moveFocus(dir, root) {
  const all = focusables(root);
  const cur = document.activeElement;
  if (!all.length) return false;
  if (!root.contains(cur)) { all[0].focus(); all[0].scrollIntoView({ block: 'nearest' }); return true; }
  const a = cur.getBoundingClientRect();
  const ax = a.left + a.width / 2, ay = a.top + a.height / 2;
  let best = null, bestScore = Infinity;
  for (const el of all) {
    if (el === cur || el.contains(cur) || cur.contains(el)) continue;
    const b = el.getBoundingClientRect();
    const bx = b.left + b.width / 2, by = b.top + b.height / 2;
    const dx = bx - ax, dy = by - ay;
    let main, cross;
    if (dir === 'up') { if (b.bottom > a.top + 2) continue; main = -dy; cross = Math.abs(dx); }
    else if (dir === 'down') { if (b.top < a.bottom - 2) continue; main = dy; cross = Math.abs(dx); }
    else if (dir === 'left') { if (b.right > a.left + 2) continue; main = -dx; cross = Math.abs(dy); }
    else { if (b.left < a.right - 2) continue; main = dx; cross = Math.abs(dy); }
    const overlap = dir === 'up' || dir === 'down'
      ? Math.min(a.right, b.right) - Math.max(a.left, b.left)
      : Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top);
    const score = main + cross * (overlap > 0 ? 0.3 : 2.5);
    if (score < bestScore) { bestScore = score; best = el; }
  }
  if (!best) return false;
  best.focus({ preventScroll: true });
  best.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  return true;
}

function typing(el) {
  return el && (el.tagName === 'TEXTAREA' || el.tagName === 'SELECT' || (el.tagName === 'INPUT' && !['range', 'checkbox', 'button'].includes(el.type)));
}

document.addEventListener('keydown', (e) => {
  if (!session || e.altKey || e.ctrlKey || e.metaKey) return;
  const el = document.activeElement;
  const dirs = { ArrowUp: 'up', ArrowDown: 'down', ArrowLeft: 'left', ArrowRight: 'right' };
  if (modalClose) {
    if (e.key === 'Escape') { e.preventDefault(); modalClose(null); return; }
    if (dirs[e.key] && (!typing(el) || e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
      if (el && el.tagName === 'SELECT') return;
      e.preventDefault();
      moveFocus(dirs[e.key], $('#scrim'));
    }
    return;
  }
  if (typing(el)) {
    if (e.key === 'Escape') el.blur();
    return;
  }
  const inRail = el && el.closest('#rail');
  if (dirs[e.key]) {
    const d = dirs[e.key];
    /* A slider keeps left/right for its value. */
    if (el && el.type === 'range' && (d === 'left' || d === 'right')) return;
    e.preventDefault();
    if (inRail) {
      if (d === 'right') { focusPane(); return; }
      const items = $$('.rail-item');
      const i = items.indexOf(el);
      const next = items[i + (d === 'down' ? 1 : d === 'up' ? -1 : 0)];
      if (next && next !== el) { next.focus(); go(next.dataset.route, { preview: true }); }
      return;
    }
    if (!moveFocus(d, $('#page')) && d === 'left') focusRail();
    disarmIfMoved();
    return;
  }
  if (e.key === 'Escape' || e.key === 'Backspace') {
    e.preventDefault();
    back();
  }
});

function disarmIfMoved() {
  if (armedKey && document.activeElement?.dataset.key !== armedKey) disarm();
}

document.addEventListener('click', (e) => {
  if (armedKey && !e.target.closest(`[data-key="${CSS.escape(armedKey)}"]`)) disarm();
});

function focusRail() {
  const a = $('.rail-item.active') || $('.rail-item');
  if (a) a.focus();
}

function focusPane() {
  const f = focusables($('#page'))[0];
  if (f) { f.focus({ preventScroll: true }); f.scrollIntoView({ block: 'nearest' }); }
}

/* ------------------------------------------------------------ routing */

/* Pages register here (pages.js): id -> { group, title(), render(path) }.
 * render returns { title, kicker, back, nodes, topics, backdrop }. */
const PAGES = {};
const RAIL = [];
const current = { path: [], topics: new Set(), building: false, again: false };

function routeOf() {
  return location.hash.replace(/^#\/?/, '').split('/').filter(Boolean).map(decodeURIComponent);
}

function go(route, { preview = false, replace = false } = {}) {
  const hash = '#/' + route.split('/').map(encodeURIComponent).join('/');
  armedKey = '';
  if (location.hash === hash) { refresh(); return; }
  if (replace) history.replaceState(null, '', hash); else location.hash = hash;
  if (!preview) document.body.classList.remove('menu');
}

function back() {
  const p = current.path;
  if (current.back) { go(current.back); return; }
  if (p.length > 1) { go(p.slice(0, -1).join('/')); return; }
  if (window.matchMedia('(max-width: 860px)').matches) { document.body.classList.add('menu'); focusRail(); return; }
  focusRail();
}

window.addEventListener('hashchange', () => { armedKey = ''; render(true); });

let refreshTimer = null;
function refresh() {
  clearTimeout(refreshTimer);
  refreshTimer = setTimeout(() => render(false), 60);
}

function refreshAll() {
  store.clear();
  topBar();
  refresh();
}

/* Re-renders keep the focused element (by data-key) and the scroll. */
async function render(navigated) {
  if (!session) return;
  if (current.building) { current.again = navigated || current.again === 'nav' ? 'nav' : true; return; }
  const active = document.activeElement;
  /* Never rebuild under the user's hands. */
  if (!navigated && active && $('#page').contains(active) && (typing(active) || active.type === 'range' && dragging)) {
    refresh();
    return;
  }
  current.building = true;
  const path = routeOf();
  const id = path[0] && PAGES[path[0]] ? path[0] : 'overview';
  const page = PAGES[id];
  let out;
  try {
    out = await page.render(path.slice(1));
  } catch (e) {
    out = { title: page.title(), nodes: [e.code === 'session' ? null : empty(C(507, 'The operation failed'), reason(e.code))], topics: [] };
  }
  current.building = false;
  if (current.again) {
    const nav = current.again === 'nav';
    current.again = false;
    render(nav);
    return;
  }
  const key = active && active.dataset ? active.dataset.key : null;
  const inPane = active && $('#page').contains(active);
  const pane = $('#pane');
  const scroll = pane.scrollTop;
  current.path = [id, ...path.slice(1)];
  current.topics = new Set(out.topics || []);
  current.back = out.back || null;
  const titleRow = h('div', { class: 'page-title' },
    out.back || path.length > 1 ? h('button', { class: 'back', type: 'button', 'aria-label': C(186, 'Back'), data: { key: '__back' }, on: { click: back } }, icon('chevron-down')) : null,
    h('div', { class: 'tw' }, out.kicker ? h('div', { class: 'kicker', text: out.kicker }) : null, h('h2', { text: out.title || page.title() })),
    out.tools || null);
  $('#page').replaceChildren(titleRow, ...list(out.nodes));
  document.title = (out.title || page.title()) + ' · nuubOS';
  $$('.rail-item').forEach((r) => r.classList.toggle('active', r.dataset.route.split('/')[0] === id));
  document.body.classList.toggle('has-backdrop', !!out.backdrop);
  let bd = $('.backdrop');
  if (out.backdrop) {
    if (!bd) { bd = h('div', { class: 'backdrop' }); document.body.append(bd); }
    bd.style.backgroundImage = `url("${out.backdrop}")`;
  } else if (bd) bd.remove();
  if (navigated) {
    pane.scrollTop = 0;
    if (!(active && active.closest('#rail'))) focusPane();
  } else {
    pane.scrollTop = scroll;
    if (key && inPane) {
      const again = $$('[data-key]', $('#page')).find((e) => e.dataset.key === key);
      if (again) again.focus({ preventScroll: true });
    }
  }
}

let dragging = false;
document.addEventListener('pointerdown', (e) => { if (e.target.type === 'range') dragging = true; });
document.addEventListener('pointerup', () => { dragging = false; });

/* ------------------------------------------------------------- shell */

function renderShell() {
  $('#rail-title').textContent = T('console');
  const rail = $('#rail');
  rail.replaceChildren();
  let lastGroup = null;
  for (const id of RAIL) {
    const p = PAGES[id];
    if (p.group !== lastGroup) {
      lastGroup = p.group;
      rail.append(h('div', { class: 'rail-group', text: p.group === 'settings' ? C(3, 'Settings') : T('manage') }));
    }
    rail.append(h('button', { class: 'rail-item', type: 'button', data: { route: id }, text: p.title(), on: { click: () => go(id) } }));
  }
  const hints = $('#hints');
  const hint = (keys, label, on) => h(on ? 'button' : 'span', { class: 'hint' + (on ? '' : ' kb'), type: on ? 'button' : null, on: on ? { click: on } : null },
    ...keys.map((k) => h('span', { class: 'key' }, typeof k === 'string' ? k : k)), label);
  hints.replaceChildren(
    hint(['↑↓←→'], C(184, 'Navigate')),
    hint(['Enter'], C(185, 'Select')),
    hint(['Esc'], C(186, 'Back')),
    h('span', { class: 'sp' }),
    hint([icon('power-off')], C(188, 'POWER MENU').toLowerCase().replace(/(^|\s)\S/g, (s) => s.toUpperCase()), powerMenu),
    hint([icon('right-from-bracket')], C(567, 'Sign Out'), signOut));
  $$('.brand-mark').forEach((m) => m.replaceChildren(logoSvg()));
  const mb = $('#menu-button');
  mb.replaceChildren(icon('chevron-down'));
  mb.onclick = () => { document.body.classList.toggle('menu'); };
}

async function powerMenu() {
  const v = await choose(C(188, 'POWER MENU'), [
    { value: 'sleep', label: C(189, 'Sleep') },
    { value: 'restart', label: C(190, 'Restart') },
    { value: 'poweroff', label: C(58, 'Power Off') },
  ]);
  if (!v) return;
  const ok = await form(v === 'sleep' ? C(189, 'Sleep') : v === 'restart' ? C(190, 'Restart') : C(58, 'Power Off'), [], { text: T('power_confirm'), ok: C(82, 'Confirm') });
  if (ok) act('power.' + v).then(() => toast(T('power_sent'))).catch(() => {});
}

const BATTERY = [[90, 'battery-full'], [62, 'battery-three-quarters'], [37, 'battery-half'], [12, 'battery-quarter'], [-1, 'battery-empty']];

async function topBar() {
  if (!session) return;
  const [st, users] = await Promise.all([get('status').catch(() => ({})), get('users').catch(() => ({}))]);
  $('#clock').textContent = st.TIME || '';
  const me = rows(users.user).find((u) => u[0] === users.active);
  const av = $('#top-avatar');
  av.style.backgroundImage = me && me[3] ? `url("${img(me[3])}")` : '';
  av.title = me ? me[1] : '';
  const s = $('#top-status');
  s.replaceChildren();
  const wifi = st.WIFI_STATE || '';
  if (wifi && wifi !== 'disabled' && wifi !== 'off') s.append(icon('wifi', wifi === 'connected' ? '' : 'dim'));
  const pct = Number(st.BATTERY_PERCENT);
  if (!Number.isNaN(pct) && st.BATTERY_PERCENT !== undefined) {
    const name = BATTERY.find(([lim]) => pct > lim)[1];
    const charging = st.BATTERY_STATE === 'charging' || st.BATTERY_STATE === 'full';
    s.append(h('span', { class: 'bat' }, icon(name, st.BATTERY_SAVER === '1' ? 'saver' : ''), charging ? icon('bolt', 'bolt') : null), h('span', { text: pct + '%' }));
  }
}

/* ------------------------------------------------------------ sign-in */

let session = false;

function showLogin(message = '') {
  session = false;
  stopEvents();
  for (const id of ['top', 'app', 'hints']) $('#' + id).hidden = true;
  $('#login').hidden = false;
  $$('.brand-mark').forEach((m) => m.replaceChildren(logoSvg()));
  $('#login-tag').textContent = C(603, 'Web Administration');
  $('#login-user-label').textContent = C(285, 'Username');
  $('#login-password-label').textContent = C(172, 'Password');
  $('#login-submit').textContent = C(587, 'Sign In');
  $('#login-note').textContent = T('login_note');
  $('#login-error').textContent = message;
  $('#login-password').value = '';
  $('#login-password').focus();
}

function signedOut() {
  if (!session) return;
  showLogin(T('session_ended'));
}

async function signOut() {
  try { await request('/api/logout', { method: 'POST', headers: { 'X-NuubOS': '1' } }); } catch (e) { /* gone anyway */ }
  store.clear();
  showLogin();
}

$('#login-form').addEventListener('submit', async (e) => {
  e.preventDefault();
  const err = $('#login-error');
  err.textContent = '';
  $('#login-submit').disabled = true;
  try {
    await request('/api/login', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json', 'X-NuubOS': '1' },
      body: JSON.stringify({ user: $('#login-user').value.trim(), password: $('#login-password').value }),
    });
    await enter();
  } catch (x) {
    err.textContent = x.code === 'locked' ? T('locked') : x.code === 'credentials' ? T('wrong_credentials') : T('offline');
  } finally {
    $('#login-submit').disabled = false;
  }
});

async function enter() {
  await loadLanguage();
  session = true;
  $('#login').hidden = true;
  for (const id of ['top', 'app', 'hints']) $('#' + id).hidden = false;
  if (!location.hash) history.replaceState(null, '', '#/overview');
  loadMood();
  renderShell();
  startEvents();
  topBar();
  await render(true);
}

async function boot() {
  /* Before sign-in: the console's Mood and language for the login page. */
  try {
    const p = await request('/api/public');
    applyMood(p.mood);
    LANG = WEB[p.lang] ? p.lang : 'en';
    document.documentElement.lang = LANG;
  } catch (e) { /* defaults */ }
  try {
    await request('/api/session');
    await enter();
  } catch (e) {
    showLogin();
  }
}

window.addEventListener('DOMContentLoaded', boot);
