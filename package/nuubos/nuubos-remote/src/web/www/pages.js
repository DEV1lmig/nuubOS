'use strict';
/* Pages of the nuubOS Web administration. Settings mirror the console's
 * categories, sections and row order (docs/dev/ui.md "Settings
 * structure"), with the console's own strings; content pages (Overview,
 * Library, Files, BIOS Files) are what a browser does better: uploads,
 * downloads, large lists. */

const ROMS = '/userdata/roms';
const BIOS = '/userdata/bios';
const LANGUAGES = [['en', 'English'], ['it', 'Italiano'], ['fr', 'Français'], ['de', 'Deutsch'], ['es', 'Español'], ['pt', 'Português'], ['nl', 'Nederlands']];

function page(id, group, title, render) {
  PAGES[id] = { group, title, render };
  RAIL.push(id);
}

const on01 = (v) => v === '1' || v === 1 || v === true;
const pct = (v) => v + '%';

/* ----------------------------------------------------------- uploads */

/* Files of a drop (folders included, with their relative folder) or of a
 * file picker. */
async function droppedFiles(dt) {
  const out = [];
  const entries = [...(dt.items || [])].map((i) => i.webkitGetAsEntry && i.webkitGetAsEntry()).filter(Boolean);
  if (!entries.length) return [...dt.files].map((file) => ({ file, rel: '' }));
  const walk = async (entry, rel) => {
    if (entry.isFile) {
      const file = await new Promise((res, rej) => entry.file(res, rej));
      out.push({ file, rel });
    } else if (entry.isDirectory) {
      const reader = entry.createReader();
      let batch;
      do {
        batch = await new Promise((res, rej) => reader.readEntries(res, rej));
        for (const e of batch) await walk(e, rel ? rel + '/' + entry.name : entry.name);
      } while (batch.length);
    }
  };
  for (const e of entries) await walk(e, '');
  return out.filter((x) => !x.file.name.startsWith('.'));
}

/* A drop zone with a multi-file picker; onFiles([{ file, rel }]). */
function dropZone(title, detail, onFiles, { accept = '', folders = true } = {}) {
  const input = h('input', { type: 'file', multiple: true, accept: accept || null, hidden: true });
  input.addEventListener('change', () => {
    const files = [...input.files].map((file) => ({ file, rel: '' }));
    input.value = '';
    if (files.length) onFiles(files);
  });
  const pick = button(T('choose_files'), () => input.click(), { icon: 'upload', key: 'pick:' + title });
  const zone = h('div', { class: 'drop' }, icon('upload'),
    h('div', { class: 'dt' }, h('b', { text: title }), h('span', { text: detail })), pick, input);
  zone.addEventListener('dragover', (e) => { e.preventDefault(); zone.classList.add('over'); });
  zone.addEventListener('dragleave', () => zone.classList.remove('over'));
  zone.addEventListener('drop', async (e) => {
    e.preventDefault();
    zone.classList.remove('over');
    const files = await droppedFiles(e.dataTransfer);
    onFiles(folders ? files : files.map((f) => ({ file: f.file, rel: '' })));
  });
  return zone;
}

async function sendAll(items, dirOf, opts) {
  const map = new Map(items.map((x) => [x.file, x]));
  return uploadFiles(items.map((x) => x.file), (f) => dirOf(map.get(f)), { mkdirs: true, ...opts });
}

/* -------------------------------------------------------- shared bits */

function gameCard(g, from) {
  /* game fields: id system title cover aspect last time available fav year players genre */
  const [id, , title, cover, , , , avail] = g;
  const c = h('button', { class: 'card' + (avail === '0' ? ' na' : ''), type: 'button', data: { key: 'g:' + id }, title,
    on: { click: () => go('library/game/' + id + (from ? '/' + from : '')) } },
  cover ? h('img', { src: img(cover), alt: '', loading: 'lazy' }) : null, h('span', { class: 'ct', text: title }));
  return c;
}

function showcase(key, name, sub, color, previews, glyph, onClick) {
  const b = h('button', { class: 'show', type: 'button', data: { key }, vars: color ? { '--c': color } : null, on: { click: onClick } },
    h('span', { class: 'wash' }),
    previews.length ? h('span', { class: 'fan' }, previews.slice(0, 3).map((p) => h('img', { src: img(p), alt: '', loading: 'lazy' })))
      : glyph ? h('img', { class: 'glyph', src: img(glyph), alt: '' }) : null,
    h('span', { class: 'lab' }, h('b', { text: name }), h('span', { text: sub })));
  return b;
}

const JOB_NAMES = {
  'support-bundle': () => C(595, 'Diagnostic report'),
  'scrape-bulk': () => C(571, 'Getting metadata'),
  'scrape-game': () => C(571, 'Getting metadata'),
  'file-copy': () => C(623, 'Copying files'),
  'file-move': () => C(624, 'Moving files'),
  'sd2-backup': () => C(982, 'Backup'),
  'update-download': () => C(660, 'Downloading update'),
  'update-install': () => C(984, 'Installing update'),
};

function jobRow(j) {
  /* job: id type state progress user step result reason */
  const [id, type, state, progress, , step, result] = j;
  const name = (JOB_NAMES[type] || (() => type))();
  const states = { running: C(91, 'Running'), queued: T('queued'), succeeded: C(92, 'Completed'), failed: C(93, 'Failed'), cancelled: C(596, 'Cancelled') };
  const live = state === 'running' || state === 'queued';
  const p = Number(progress);
  return row({
    key: 'job:' + id, title: name, icon: live ? 'circle-notch' : state === 'succeeded' ? 'check' : 'triangle-exclamation',
    detail: [states[state] || state, step].filter(Boolean).join(' • '),
    value: live && p >= 0 ? p + '%' : '',
    on: live ? () => act('job.cancel', { id }) : state === 'failed' ? () => act('job.retry', { id }) : result && result.startsWith('/userdata/') ? () => { location.href = '/api/download?' + new URLSearchParams({ p: result }); } : null,
    arm: live ? { prompt: C(482, 'Press to cancel') } : null,
  });
}

/* ========================================================= OVERVIEW */

page('overview', 'manage', () => T('overview'), async () => {
  const [sys, lib, emu, jobs, roots] = await Promise.all([get('system'), get('library'), get('emu'), get('jobs'), get('roots').catch(() => ({}))]);
  const nodes = [];
  if (emu.state && emu.state !== 'idle' && emu.game) {
    let d = null;
    try { d = await get('details', { id: emu.game }); } catch (e) { /* unknown game */ }
    const g = d ? rows(d.game)[0] : null;
    nodes.push(h('div', { class: 'now' }, g && g[3] ? h('img', { src: img(g[3]), alt: '' }) : null,
      h('div', { class: 'rt' }, h('div', { class: 'kicker', text: C(369, 'Now Playing') }), h('div', { class: 'title', text: g ? g[2] : emu.game }),
        h('div', { class: 'detail', text: d ? d.system_name : emu.system })),
      button(C(422, 'Quit Game'), () => act('emu.quit', {}, { topics: ['emu'] }), { key: 'quit', arm: { prompt: C(191, 'Press again to confirm') } })));
  }
  const battery = Number(sys['battery.percent']);
  const bstate = sys['battery.state'];
  const root = rows(roots.root).find((r) => r[1] === '/userdata') || rows(roots.root)[0];
  const total = root ? Number(root[2]) : 0, free = root ? Number(root[3]) : 0;
  const tile = (label, ic, value, sub, meter) => h('div', { class: 'tile' }, h('div', { class: 'tl' }, icon(ic), label),
    h('div', { class: 'tv', text: value }), meter !== undefined ? h('div', { class: 'meter' }, h('i', { vars: { width: meter + '%' } })) : null, h('div', { class: 'ts', text: sub }));
  const meterTile = (el, f) => { const i = $('.meter i', el); if (i) i.style.width = f + '%'; return el; };
  nodes.push(h('div', { class: 'hero' },
    meterTile(tile(C(100, 'Battery'), 'battery-full', Number.isNaN(battery) ? '–' : battery + '%',
      bstate === 'charging' ? C(382, 'Charging') : bstate === 'full' ? C(383, 'Fully charged') : C(818, 'Discharging'), 0), battery),
    meterTile(tile(C(19, 'Storage'), 'sd-card', total ? sizeText(free) : '–', total ? C(622, '{0} free', sizeText(free)) + ' / ' + sizeText(total) : '', 0), total ? (1 - free / total) * 100 : 0),
    tile(C(149, 'Games'), 'gamepad', lib.games || '0', lib.scanning === '1' ? C(410, 'Searching for games…') : C(840, 'Systems') + ': ' + rows(lib.system).length),
    tile(C(103, 'Network'), 'wifi', (sys.network || '').split('•').pop().trim() || C(159, 'Not connected'), sys.network ? (sys.network.split('•')[0] || '').trim() : ''),
    tile(C(98, 'Device'), 'microchip', 'nuubOS ' + (sys['nuubos.version'] || ''), sys['device.model'] || '')));

  const recent = rows(lib.recent);
  nodes.push(section(C(0, 'Recently Played'), 'user'));
  nodes.push(recent.length ? h('div', { class: 'strip' }, recent.map((g) => gameCard(g))) : h('div', { class: 'notice', text: C(715, 'Your adventure log is a blank page. Pick a game and write chapter one!') }));

  const all = rows(jobs.job);
  const live = all.filter((j) => j[2] === 'running' || j[2] === 'queued');
  const past = all.filter((j) => !live.includes(j)).slice(-4).reverse();
  if (all.length) {
    nodes.push(section(T('tasks'), 'device'));
    nodes.push(group([...live, ...past].map(jobRow)));
  }
  nodes.push(h('div', { class: 'actions' },
    button(C(804, 'Scan for New Games'), () => act('lib.scan').then(() => toast(C(410, 'Searching for games…'))), { icon: 'magnifying-glass', key: 'scan' }),
    button(T('upload_games'), () => go('library/upload'), { icon: 'upload', key: 'up' })));
  return { title: T('overview'), nodes, topics: ['system', 'library', 'emu', 'jobs', 'status'] };
});

/* ========================================================== LIBRARY */

async function systemsRegistry() {
  return (await get('systems')).systems || [];
}

page('library', 'manage', () => T('library'), async (path) => {
  const [what, arg, from] = path;
  if (what === 'game') return gamePage(arg, path[2]);
  if (what === 'system') return systemPage(arg);
  if (what === 'collection') return collectionPage(arg);
  if (what === 'search') return searchPage(arg || '');
  if (what === 'upload') return uploadPage();
  const lib = await get('library');
  const search = h('input', { class: 'input', type: 'search', placeholder: C(831, 'Title'), 'aria-label': C(828, 'Find'), data: { key: 'search' } });
  search.addEventListener('keydown', (e) => { if (e.key === 'Enter' && search.value.trim()) go('library/search/' + search.value.trim()); });
  const nodes = [
    h('div', { class: 'searchbar' }, h('div', { class: 'wrap' }, icon('magnifying-glass'), search),
      button(T('upload_games'), () => go('library/upload'), { icon: 'upload', key: 'upload' })),
  ];
  const systems = rows(lib.system);
  nodes.push(section(C(840, 'Systems'), 'device'));
  nodes.push(systems.length
    ? h('div', { class: 'shelf' }, systems.map((s) => showcase('s:' + s[0], s[1], gamesText(s[2]), s[4], s.slice(6).filter(Boolean), s[5], () => go('library/system/' + s[0]))))
    : empty(C(398, 'No games yet'), C(2, 'Copy your games to the roms folder on the SD card, one folder per system.')));
  nodes.push(section(C(537, 'Collections'), 'user'));
  const cols = rows(lib.collection);
  nodes.push(h('div', { class: 'shelf' },
    cols.map((c) => showcase('c:' + c[0], c[1], gamesText(c[2]), null, c.slice(3).filter(Boolean), null, () => go('library/collection/' + c[0]))),
    h('button', { class: 'show new', type: 'button', data: { key: 'newcol' }, on: { click: newCollection } }, h('span', { class: 'lab' }, h('b', { text: C(538, 'New Collection') })))));
  return { title: T('library'), nodes, topics: ['library'] };
});

async function newCollection(game) {
  const v = await form(C(538, 'New Collection'), [{ name: 'name', label: C(539, 'Collection name') }]);
  if (!v || !v.name.trim()) return null;
  await act('coll.create', { name: v.name.trim() }, { topics: ['library'] });
  if (typeof game === 'string') {
    const lib = await get('library', {}, true);
    const c = rows(lib.collection).find((x) => x[1] === v.name.trim());
    if (c) await act('coll.add', { id: c[0], game });
  }
  return v.name;
}

async function systemPage(id) {
  const [games, registry, lib] = await Promise.all([get('games', { scope: 'system:' + id }), systemsRegistry(), get('library')]);
  const sys = registry.find((s) => s.id === id) || { id, name: id, folder: id, extensions: '' };
  const list = rows(games.game);
  const exts = sys.extensions.split(',').filter(Boolean);
  const nodes = [dropZone(T('upload_to', sys.name), T('file_types', exts.map((e) => '.' + e).join(' ')), (items) => {
    const bad = items.filter((x) => !exts.some((e) => x.file.name.toLowerCase().endsWith('.' + e)) && !/\.(cue|bin|m3u|img|iso|chd|ccd|sub)$/i.test(x.file.name));
    if (bad.length) toast(T('not_for_system', bad.length), bad.slice(0, 3).map((x) => x.file.name).join(', '), true, 6000);
    const good = items.filter((x) => !bad.includes(x));
    sendAll(good, (x) => ROMS + '/' + sys.folder + (x.rel ? '/' + x.rel : ''), { after: 'scan' });
  })];
  nodes.push(list.length ? h('div', { class: 'grid' }, list.map((g) => gameCard(g))) : empty(C(398, 'No games yet'), ''));
  const sysLib = rows(lib.system).find((s) => s[0] === id);
  return { title: sys.name, kicker: gamesText(list.length), back: 'library', nodes, topics: ['library'], tools: sysLib && sysLib[5] ? h('img', { class: 'avatar', src: img(sysLib[5]), alt: '' }) : null };
}

async function collectionPage(id) {
  const [games, lib] = await Promise.all([get('games', { scope: 'collection:' + id }), get('library')]);
  const col = rows(lib.collection).find((c) => c[0] === id);
  const list = rows(games.game);
  const tools = h('div', { class: 'actions' },
    button(C(843, 'Rename Collection'), async () => {
      const v = await form(C(843, 'Rename Collection'), [{ name: 'name', label: C(539, 'Collection name'), value: col ? col[1] : '' }]);
      if (v && v.name.trim()) await act('coll.rename', { id, name: v.name.trim() }, { topics: ['library'] });
    }, { icon: 'pen', round: true, key: 'rename' }),
    button(C(839, 'Delete Collection'), async () => {
      await act('coll.delete', { id }, { topics: ['library'] });
      toast(C(848, 'Collection deleted'));
      go('library');
    }, { icon: 'trash-can', round: true, cls: 'danger', key: 'delcol', arm: { prompt: C(191, 'Press again to confirm') } }));
  const nodes = [list.length ? h('div', { class: 'grid' }, list.map((g) => gameCard(g, 'c-' + id))) : empty(C(398, 'No games yet'), C(850, 'Games stay in your library; only the collection goes away'))];
  if (armedKey === 'delcol') nodes.unshift(h('div', { class: 'notice', text: C(850, 'Games stay in your library; only the collection goes away') }));
  return { title: col ? col[1] : C(537, 'Collections'), kicker: gamesText(list.length), back: 'library', nodes, tools, topics: ['library'] };
}

async function searchPage(q) {
  const words = q.replace(/[|\t]/g, ' ').trim();
  const r = await get('search', { spec: 'q=' + words });
  const list = rows(r.game);
  const search = h('input', { class: 'input', type: 'search', value: words, 'aria-label': C(828, 'Find'), data: { key: 'search' } });
  search.addEventListener('keydown', (e) => { if (e.key === 'Enter') go('library/search/' + search.value.trim(), { replace: true }); });
  return {
    title: C(837, 'Results'), kicker: gamesText(list.length), back: 'library', topics: ['library'],
    nodes: [h('div', { class: 'searchbar' }, h('div', { class: 'wrap' }, icon('magnifying-glass'), search)),
      list.length ? h('div', { class: 'grid' }, list.map((g) => gameCard(g))) : empty(C(836, 'No games match these filters'), '')],
  };
}

async function uploadPage() {
  const [registry, lib] = await Promise.all([systemsRegistry(), get('library')]);
  const counts = Object.fromEntries(rows(lib.system).map((s) => [s[0], s[2]]));
  const sorted = [...registry].sort((a, b) => a.name.localeCompare(b.name, LANG));
  return {
    title: T('upload_games'), back: 'library', topics: ['library'],
    nodes: [h('div', { class: 'notice', text: T('upload_pick_system') }),
      group(sorted.map((s) => row({ key: 'u:' + s.id, title: s.name, detail: T('file_types', s.extensions.split(',').map((e) => '.' + e).join(' ')),
        value: counts[s.id] ? gamesText(counts[s.id]) : '', chevron: true, on: () => go('library/system/' + s.id) })))],
  };
}

async function gamePage(id, from) {
  const d = await get('details', { id }, true);
  const g = rows(d.game)[0] || [];
  const [, system, title, cover, , last, time, avail, , year, players] = g;
  const meta = (k) => d['meta_' + k] || '';
  const genre = meta('genre') || g[11] || '';
  const backTo = from && from.startsWith('c-') ? 'library/collection/' + from.slice(2) : 'library/system/' + system;
  const chips = [];
  if (players || meta('players')) chips.push(h('span', {}, icon('user-group'), meta('players') || players));
  if (meta('rating')) chips.push(h('span', {}, icon('star'), meta('rating')));
  chips.push(h('span', {}, icon('clock'), Number(last) ? agoText(last) + ' • ' + playtimeText(time) : C(405, 'Never played')));
  const members = rows(d.member);
  const actions = h('div', { class: 'actions' },
    button(C(838, 'Add to Collection'), async () => {
      const opts = members.map((m) => ({ value: m[0], label: m[1], detail: m[2] === '1' ? C(222, 'On') : C(54, 'Off') }));
      opts.push({ value: '__new', label: C(538, 'New Collection') });
      const v = await choose(C(537, 'Collections'), opts);
      if (!v) return;
      if (v === '__new') { const n = await newCollection(id); if (n) toast(C(846, 'Added to {0}', n)); return; }
      const m = members.find((x) => x[0] === v);
      const adding = m[2] !== '1';
      await act(adding ? 'coll.add' : 'coll.remove', { id: v, game: id }, { topics: ['details', 'library'] });
      toast(adding ? C(846, 'Added to {0}', m[1]) : C(847, 'Removed from {0}', m[1]));
    }, { icon: 'layer-group', round: true, key: 'col' }),
    button(meta('match') || meta('title') ? C(560, 'Update Metadata') : C(559, 'Get Metadata'), async () => {
      await act('scraper.game', { id, mode: meta('match') ? 'force' : '' });
      toast(C(571, 'Getting metadata'), title);
    }, { icon: 'rotate', round: true, key: 'meta' }),
    button(C(524, 'Game Settings'), () => go('library/gamesettings/' + id + '/' + system), { icon: 'sliders', round: true, key: 'gs' }),
    button(T('download'), () => { location.href = '/api/download?' + new URLSearchParams({ p: ROMS + '/' + d.path }); }, { icon: 'download', round: true, key: 'dl', disabled: avail === '0' }),
    button(C(549, 'Reset Play Statistics'), () => act('lib.statsreset', { id }, { topics: ['details', 'library'] }), { icon: 'rotate-left', round: true, key: 'stats', arm: { prompt: C(598, 'Press again to reset') } }),
    button(C(544, 'Delete Game'), async () => {
      await act('lib.delete', { id }, { topics: ['library'] });
      toast(C(547, 'Game deleted'), title);
      go(backTo);
    }, { icon: 'trash-can', round: true, cls: 'danger', key: 'del', arm: { prompt: C(546, 'Press again to delete permanently') } }));
  /* The caption of the focused action, like the console. */
  const caption = h('div', { class: 'caption' });
  const captionFor = (b) => {
    const k = b.dataset.key;
    caption.textContent = armedKey === k ? (k === 'del' ? C(546, 'Press again to delete permanently') : C(598, 'Press again to reset'))
      : k === 'del' ? C(545, 'Permanently deletes the game file for every user. Saves are kept.') : b.getAttribute('aria-label') || '';
  };
  $$('.btn', actions).forEach((b) => { b.addEventListener('focus', () => captionFor(b)); b.addEventListener('mouseenter', () => captionFor(b)); });
  const info = [
    [C(558, 'File'), d.path], [T('size'), sizeText(d.size)], [T('added'), dateText(d.added, false)],
    [C(550, 'Sessions'), d.sessions], [C(551, 'Last Played'), Number(last) ? dateText(last) : C(405, 'Never played')],
    [C(552, 'Time Played'), playtimeText(time)], [C(553, 'Developer'), meta('developer')], [C(554, 'Publisher'), meta('publisher')],
    [C(555, 'Release Date'), meta('release')], [C(556, 'Genre'), genre], [C(557, 'Players'), meta('players') || (Number(players) ? players : '')],
  ].filter(([, v]) => v);
  const nodes = [
    h('div', { class: 'details' },
      cover ? h('img', { class: 'cover', src: img(cover), alt: '' }) : h('div', { class: 'cover' }),
      h('div', {}, h('div', { class: 'kicker', text: d.system_name || system }), h('h2', { text: title }),
        h('div', { class: 'meta', text: [Number(year) ? year : '', genre, meta('developer')].filter(Boolean).join('  •  ') }),
        h('div', { class: 'chips' }, chips), meta('description') ? h('div', { class: 'desc', text: meta('description').replace(/\\n/g, '\n') }) : null,
        actions, caption)),
    section(C(523, 'Details'), null),
    group(info.map(([k, v]) => row({ key: 'i:' + k, title: k, value: String(v) }))),
  ];
  return { title: C(522, 'Game Details'), back: backTo, nodes, topics: ['library'], backdrop: cover ? img(cover) : null };
}

const libraryRender = PAGES.library.render;
PAGES.library.render = async (path) => {
  if (path[0] === 'gamesettings') return gameSettingsPage(path[1], path[2]);
  return libraryRender(path);
};

async function gameSettingsPage(id, system) {
  const s = await get('game_settings', { id, system }, true);
  const def = (v) => C(528, 'Default ({0})', v);
  const aspects = { core: C(529, 'As the game'), '4:3': '4:3', '16:9': '16:9', full: C(530, 'Full Screen'), integer: C(531, 'Integer Scaling') };
  const filters = { sharp: C(532, 'Sharp'), smooth: C(533, 'Smooth'), pixel: C(534, 'Pixel Perfect'), crt: C(777, 'CRT Effect') };
  const cores = (s.cores || '').split(/[ ,]/).filter(Boolean);
  const set = (key, value) => act('game.set', { id, system, key, value }, { topics: ['game_settings'] });
  const pick = async (key, title, options, cur) => { const v = await choose(title, options, cur); if (v !== null) await set(key, v); };
  return {
    title: C(524, 'Game Settings'), back: 'library/game/' + id, topics: [],
    nodes: [group(
      row({ key: 'core', title: C(525, 'Emulator'), value: s.core_override || def(s.core_default), chevron: true, disabled: cores.length < 2,
        on: () => pick('core', C(525, 'Emulator'), [{ value: '', label: def(s.core_default) }, ...cores.map((c) => ({ value: c, label: c }))], s.core_override) }),
      row({ key: 'aspect', title: C(526, 'Aspect Ratio'), value: s.aspect ? aspects[s.aspect] || s.aspect : def(C(531, 'Integer Scaling')), chevron: true,
        on: () => pick('aspect', C(526, 'Aspect Ratio'), [{ value: '', label: def(C(531, 'Integer Scaling')) }, ...Object.entries(aspects).map(([value, label]) => ({ value, label }))], s.aspect) }),
      row({ key: 'filter', title: C(527, 'Video Filter'), value: s.filter ? filters[s.filter] : def(filters[s.filter_default] || ''), chevron: true,
        on: () => pick('filter', C(527, 'Video Filter'), [{ value: '', label: def(filters[s.filter_default] || '') }, ...['sharp', 'smooth', 'pixel'].map((v) => ({ value: v, label: filters[v] }))], s.filter) }),
      row({ key: 'reset', title: C(535, 'Reset Game Settings'), detail: C(536, 'Changes apply the next time the game starts'),
        arm: { prompt: C(598, 'Press again to reset') }, on: () => act('game.reset', { id }, { topics: ['game_settings'] }) }))],
  };
}

/* ============================================================ FILES */

let clipboard = null;

const FILE_ICONS = [[/\.(png|jpe?g|gif|webp|bmp)$/i, 'image'], [/\.(mp3|ogg|flac|wav|m4a|opus)$/i, 'music'], [/\.(mp4|mkv|avi|webm|mov)$/i, 'film'],
  [/\.(zip|7z|tar|gz|rar)$/i, 'box-archive']];

page('files', 'manage', () => C(609, 'Files'), async (path) => {
  if (path[0] === ':shares') return sharesPage();
  if (!path.length) {
    const [r, media] = await Promise.all([get('roots', {}, true), get('media').catch(() => ({}))]);
    const roots = rows(r.root);
    const ejectable = new Set(rows(media.volume).map((v) => v[0]));
    return {
      title: C(609, 'Files'), topics: ['roots'],
      nodes: [group(roots.map((x) => {
        const [name, p, total, free, removable] = x;
        const label = name === 'userdata' ? C(610, 'Internal Storage') : name;
        return h('div', { class: 'rows' }, row({ key: 'r:' + p, title: label, icon: removable === '1' || ejectable.has(name) ? 'hard-drive' : p.startsWith('/run') ? 'server' : 'sd-card',
          detail: Number(total) ? C(622, '{0} free', sizeText(free)) + ' / ' + sizeText(total) : '', chevron: true, on: () => go('files' + p) }));
      }), row({ key: 'shares', title: C(611, 'Network Shares'), icon: 'network-wired', chevron: true, on: () => go('files/:shares') }))],
    };
  }
  const abs = '/' + path.join('/');
  const r = await get('files', { path: abs }, true);
  const entries = rows(r.entry).sort((a, b) => (a[0] === b[0] ? a[3].localeCompare(b[3], LANG, { numeric: true }) : a[0] === 'd' ? -1 : 1));
  const crumbs = h('div', { class: 'crumbs' }, path.map((seg, i) => [i ? '/' : null,
    h('button', { type: 'button', text: i === 0 && seg === 'userdata' ? C(610, 'Internal Storage') : seg, on: { click: () => go('files/' + path.slice(0, i + 1).join('/')) } })]));
  const tools = h('div', { class: 'actions' },
    button(C(612, 'New Folder'), async () => {
      const v = await form(C(612, 'New Folder'), [{ name: 'name', label: C(613, 'Folder name') }]);
      if (v && v.name.trim()) await act('files.mkdir', { dir: abs, name: v.name.trim() });
    }, { icon: 'folder-plus', round: true, key: 'mkdir' }),
    clipboard ? button(C(614, 'Paste Here'), async () => {
      await act(clipboard.op === 'move' ? 'files.move' : 'files.copy', { path: clipboard.path, dir: abs });
      toast(clipboard.op === 'move' ? C(624, 'Moving files') : C(623, 'Copying files'), clipboard.path.split('/').pop());
      clipboard = null;
    }, { icon: 'paste', round: true, key: 'paste' }) : null);
  const nodes = [crumbs,
    dropZone(T('upload_files'), T('upload_here'), (items) => sendAll(items, (x) => abs + (x.rel ? '/' + x.rel : '')).then(refresh))];
  if (!entries.length) nodes.push(empty(C(621, 'Empty folder'), ''));
  nodes.push(group(entries.map((e) => {
    const [type, size, mtime, name] = e;
    const p = abs + '/' + name;
    const dir = type === 'd';
    const ic = dir ? 'folder' : (FILE_ICONS.find(([re]) => re.test(name)) || [null, 'file'])[1];
    return row({ key: 'e:' + name, title: name, icon: ic, image: !dir && /\.(png|jpe?g|webp|gif)$/i.test(name) ? img(p) : null,
      detail: dir ? dateText(mtime) : sizeText(size) + ' • ' + dateText(mtime), chevron: dir,
      on: () => (dir ? go('files' + p) : fileMenu(p, name, false)), more: true, path: p, name, dir });
  })));
  /* Every entry: its own menu button (folders open on click). */
  for (const el of $$('.row', nodes[nodes.length - 1])) {
    const name = el.dataset.key.slice(2);
    const p = abs + '/' + name;
    const isDir = entries.find((e) => e[3] === name)?.[0] === 'd';
    const wrap = h('div', { class: 'entry' });
    el.replaceWith(wrap);
    wrap.append(el, button(C(849, 'Manage'), () => fileMenu(p, name, isDir), { icon: 'ellipsis-vertical', round: true, key: 'm:' + name }));
  }
  return { title: path.length === 1 && path[0] === 'userdata' ? C(610, 'Internal Storage') : path[path.length - 1], back: path.length > 1 ? 'files/' + path.slice(0, -1).join('/') : 'files', nodes, tools, topics: [] };
});

async function fileMenu(p, name, dir) {
  const opts = [];
  if (!dir) opts.push({ value: 'download', label: T('download') });
  if (!dir && /\.tar$/i.test(name)) opts.push({ value: 'mood', label: C(648, 'Install Mood') });
  opts.push({ value: 'copy', label: C(615, 'Copy') }, { value: 'move', label: C(616, 'Move') }, { value: 'rename', label: C(617, 'Rename') }, { value: 'delete', label: C(618, 'Delete') });
  const v = await choose(name, opts);
  if (v === 'download') location.href = '/api/download?' + new URLSearchParams({ p });
  else if (v === 'mood') { await act('mood.install', { path: p }); toast(C(649, 'Mood installed')); }
  else if (v === 'copy' || v === 'move') { clipboard = { op: v, path: p }; toast(v === 'copy' ? C(615, 'Copy') : C(616, 'Move'), T('paste_hint')); }
  else if (v === 'rename') {
    const f = await form(C(617, 'Rename'), [{ name: 'name', label: C(617, 'Rename'), value: name }]);
    if (f && f.name.trim() && f.name !== name) await act('files.rename', { path: p, name: f.name.trim() });
  } else if (v === 'delete') {
    const ok = await form(C(620, 'Delete "{0}"?', name), [], { ok: C(618, 'Delete') });
    if (ok) await act('files.delete', { path: p });
  }
  refresh();
}

async function sharesPage() {
  const [s, media] = await Promise.all([get('shares', {}, true), get('media', {}, true).catch(() => ({}))]);
  const shares = rows(s.share);
  const vols = rows(media.volume);
  return {
    title: C(611, 'Network Shares'), back: 'files', topics: ['shares'],
    nodes: [
      group(shares.map((x) => {
        const [name, addr, , mounted] = x;
        return row({ key: 'sh:' + name, title: name, detail: addr, icon: 'server', pill: { text: mounted === '1' ? C(198, 'Connected') : C(445, 'Offline'), cls: mounted === '1' ? 'on' : '' },
          on: async () => {
            const v = await choose(name, [
              mounted === '1' ? { value: 'open', label: C(414, 'Open') } : { value: 'mount', label: C(158, 'Connect') },
              mounted === '1' ? { value: 'umount', label: C(169, 'Disconnect') } : null,
              { value: 'remove', label: C(644, 'Remove') }].filter(Boolean));
            if (v === 'open') go('files/run/nuubos/shares/' + name);
            else if (v) await act('share.' + v, { name }).catch(() => toast(C(645, 'The share could not be reached'), '', true));
          } });
      }),
      row({ key: 'addshare', title: C(641, 'Add Share'), icon: 'plus', on: async () => {
        const v = await form(C(641, 'Add Share'), [{ name: 'name', label: C(642, 'Share name') }, { name: 'address', label: C(643, 'Share address (//server/share)'), placeholder: '//server/share' },
          { name: 'user', label: C(285, 'Username') }, { name: 'password', label: C(172, 'Password'), type: 'password' }]);
        if (v) await act('share.add', v);
      } })),
      section(C(625, 'Removable Media'), 'device'),
      group(vols.length ? vols.map((m) => row({ key: 'vol:' + m[0], title: m[0], icon: 'hard-drive', detail: m[1] || '', value: C(626, 'Eject'),
        on: async () => { await act('media.eject', { name: m[0] }, { topics: ['media', 'roots'] }); toast(C(627, 'Safe to remove'), m[0]); } }))
        : row({ key: 'novol', title: C(629, 'No removable media'), disabled: true })),
    ],
  };
}

/* ======================================================== BIOS FILES */

const BIOS_STATES = { ready: () => C(66, 'Ready'), ok: () => C(66, 'Ready'), missing: () => C(577, 'Missing'), invalid: () => C(578, 'Invalid'),
  optional: () => C(579, 'Optional'), unverified: () => C(580, 'Not verifiable') };
const biosPill = (s) => ({ text: (BIOS_STATES[s] || (() => s))(), cls: s === 'ready' || s === 'ok' ? 'on' : s === 'invalid' || s === 'missing' ? 'bad' : '' });

function biosModel(b) {
  const systems = rows(b.system).map((s) => ({ id: s[0], name: s[1], state: s[3], files: [] }));
  for (const f of rows(b.file)) {
    const s = systems.find((x) => x.id === f[0]);
    if (s) s.files.push({ path: f[1], required: f[2] === '1', state: f[3], description: f[4] });
  }
  return systems;
}

/* Dropped BIOS files go where the BIOS Manager expects a file of that
 * name; the manager then checks them (EPIC-033). */
function biosUpload(items, systems) {
  const wanted = new Map();
  for (const s of systems) for (const f of s.files) wanted.set(f.path.split('/').pop().toLowerCase(), f.path);
  const known = items.filter((x) => wanted.has(x.file.name.toLowerCase()));
  const unknown = items.filter((x) => !known.includes(x));
  if (unknown.length) toast(T('bios_unknown', unknown.length), unknown.slice(0, 3).map((x) => x.file.name).join(', '), true, 6000);
  if (!known.length) return;
  const byFile = new Map(known.map((x) => [x.file, wanted.get(x.file.name.toLowerCase())]));
  uploadFiles(known.map((x) => x.file), (f) => { const p = byFile.get(f); const i = p.lastIndexOf('/'); return i > 0 ? BIOS + '/' + p.slice(0, i) : BIOS; },
    { replace: true, mkdirs: true, after: 'bios' }).then(() => { forget('bios'); refresh(); });
}

page('bios', 'manage', () => C(576, 'BIOS Files'), async (path) => {
  const [b, lib] = await Promise.all([get('bios'), get('library')]);
  const systems = biosModel(b);
  const zone = dropZone(T('upload_bios'), C(581, 'Copy BIOS files to the bios folder on the SD card. nuubOS never includes them.'), (items) => biosUpload(items, systems), { folders: false });
  if (path[0]) {
    const s = systems.find((x) => x.id === path[0]);
    if (!s) return { title: C(576, 'BIOS Files'), back: 'bios', nodes: [empty(C(205, 'Unavailable'))] };
    return {
      title: s.name, kicker: C(576, 'BIOS Files'), back: 'bios', topics: ['bios'],
      nodes: [zone, group(s.files.map((f) => row({ key: 'f:' + f.path, title: f.path, detail: f.required ? f.description : C(579, 'Optional') + ' • ' + f.description,
        pill: biosPill(f.state), on: () => {
          const input = h('input', { type: 'file' });
          input.addEventListener('change', () => input.files[0] && biosUpload([{ file: new File([input.files[0]], f.path.split('/').pop()), rel: '' }], systems));
          input.click();
        } })))],
    };
  }
  const mine = new Set(rows(lib.system).map((s) => s[0]));
  const own = systems.filter((s) => mine.has(s.id)), other = systems.filter((s) => !mine.has(s.id));
  const sysRow = (s) => row({ key: 's:' + s.id, title: s.name, detail: C(582, '{0} of {1} present', s.files.filter((f) => f.state === 'ok').length, s.files.length),
    pill: biosPill(s.state), chevron: true, on: () => go('bios/' + s.id) });
  return {
    title: C(576, 'BIOS Files'), topics: ['bios'],
    nodes: [zone, section(C(760, 'YOUR SYSTEMS'), 'device'), group(own.length ? own.map(sysRow) : row({ key: 'none', title: C(398, 'No games yet'), disabled: true })),
      section(C(761, 'OTHER SYSTEMS'), 'device'), group(other.map(sysRow))],
  };
});

/* ========================================================== GENERAL */

async function pickAndAct(title, options, cur, op, key = 'v', topics = []) {
  const v = await choose(title, options, cur);
  if (v !== null) await act(op, { [key]: v }, { topics });
}

page('general', 'settings', () => C(4, 'General'), async (path) => {
  const [users, loc, reg, themes] = await Promise.all([get('users'), get('localization'), get('regional'), get('themes')]);
  const all = rows(users.user);
  const me = all.find((u) => u[0] === users.active);
  if (path[0] === 'profile') return profilePage(path[1] || users.active, all, users);
  if (path[0] === 'avatar') return avatarPage(path[1] || users.active);
  const theme = rows(themes.list).find((t) => t[0] === themes.selected);
  const nodes = [
    section(C(333, 'PROFILE'), 'user'),
    group(
      row({ key: 'profile', title: C(192, 'Profile'), image: me && me[3] ? img(me[3]) : null, value: me ? me[1] : '', chevron: true, on: () => go('general/profile') }),
      row({ key: 'lang', title: C(193, 'Language'), value: (LANGUAGES.find((l) => l[0] === loc.language) || ['', loc.language])[1], chevron: true,
        on: () => pickAndAct(C(193, 'Language'), LANGUAGES.map(([value, label]) => ({ value, label })), loc.language, 'lang.set') }),
      row({ key: 'mood', title: C(194, 'Mood'), value: theme ? theme[1] : C(195, 'Default mood'), chevron: true, on: async () => {
        const v = await choose(C(194, 'Mood'), rows(themes.list).map((t) => ({ value: t[0], label: t[1], detail: t[2] })), themes.selected);
        if (v) { await act('mood.select', { id: v }, { topics: ['themes'] }); loadMood(); }
      } }),
    ),
    dropZone(C(648, 'Install Mood'), T('mood_hint'), (items) => uploadFiles(items.map((x) => x.file), () => '/userdata/downloads', { after: 'mood' }), { accept: '.tar', folders: false }),
    section(C(334, 'USERS'), 'device'),
    group(
      all.map((u) => row({ key: 'u:' + u[0], title: u[1], image: img(u[3]),
        detail: [u[4] === '1' ? C(304, 'Active') : '', u[5] === '1' ? C(303, 'Default') : ''].filter(Boolean).join(' • '), chevron: true,
        on: () => go('general/profile/' + u[0]) })),
      row({ key: 'adduser', title: C(288, 'Add User'), icon: 'plus', on: async () => {
        const v = await form(C(295, 'Create User'), [{ name: 'name', label: C(296, 'Enter username') }]);
        if (v && v.name.trim()) { await act('user.create', { name: v.name.trim() }); toast(C(318, 'User created')); }
      } }),
      row({ key: 'login', title: C(276, 'Startup'), value: users.login_mode === 'default' ? C(278, 'Direct Login') : C(277, 'User Picker'), chevron: true,
        on: () => pickAndAct(C(276, 'Startup'), [{ value: 'select', label: C(277, 'User Picker'), detail: C(307, 'Show user picker at boot') },
          { value: 'default', label: C(278, 'Direct Login'), detail: C(308, 'Load the default user at boot') }], users.login_mode, 'user.loginmode') }),
      row({ key: 'defuser', title: C(279, 'Default User'), value: (all.find((u) => u[0] === users.default_user) || [, ''])[1], chevron: true,
        on: () => pickAndAct(C(279, 'Default User'), all.map((u) => ({ value: u[0], label: u[1] })), users.default_user, 'user.default', 'id') }),
    ),
    section(C(335, 'DATE & TIME'), 'device'),
    group(
      row({ key: 'tz', title: C(280, 'Timezone'), value: reg.timezone, chevron: true, on: async () => {
        const z = await get('timezones');
        await pickAndAct(C(280, 'Timezone'), list(z.zone).map((v) => ({ value: v, label: v.replace(/_/g, ' ') })), reg.timezone, 'tz.set');
      } }),
      row({ key: 'auto', title: C(281, 'Automatic Time'), detail: C(310, 'Use NTP when network is available'), toggle: on01(reg.automatic_time),
        set: (v) => act('time.auto', { v }) }),
      row({ key: 'clock', title: C(326, 'Date') + ' / ' + C(327, 'Time'), value: reg.local_date + ' ' + reg.local_time, disabled: on01(reg.automatic_time),
        detail: on01(reg.automatic_time) ? '' : C(322, 'Automatic time is off'), on: async () => {
          const v = await form(C(326, 'Date') + ' / ' + C(327, 'Time'), [{ name: 'd', label: C(326, 'Date'), type: 'date', value: reg.local_date },
            { name: 't', label: C(327, 'Time'), type: 'time', value: reg.local_time }]);
          if (v && v.d && v.t) await act('time.set', { v: v.d + ' ' + v.t.slice(0, 5) });
        } }),
      row({ key: 'sync', title: C(282, 'Sync Now'), detail: C(301, 'Last Sync') + ': ' + (Number(reg.last_sync) ? dateText(reg.last_sync) : C(302, 'Never')),
        disabled: !on01(reg.automatic_time), on: async () => {
          toast(C(323, 'Syncing…'));
          try { await act('time.sync', {}, { quiet: true }); toast(C(299, 'Time synchronized')); } catch (e) { toast(C(300, 'Time synchronization failed'), '', true); }
        } }),
    ),
    section(C(336, 'KEYBOARD'), 'device'),
    group(row({ key: 'kbd', title: C(283, 'Keyboard Layout'), detail: C(311, 'Physical and virtual keyboard'), value: reg.keyboard_layout, chevron: true, on: async () => {
      const k = await get('keyboards');
      await pickAndAct(C(283, 'Keyboard Layout'), rows(k.keyboard).map((r) => ({ value: r[0], label: r[1], detail: r[2] })), reg.keyboard_layout, 'kbd.set');
    } })),
  ];
  return { title: C(4, 'General'), nodes, topics: ['users', 'localization', 'regional', 'themes'] };
});

async function profilePage(id, all, users) {
  const u = all.find((x) => x[0] === id);
  if (!u) return { title: C(192, 'Profile'), back: 'general', nodes: [empty(C(205, 'Unavailable'))] };
  const active = u[0] === users.active;
  const nodes = [section(C(333, 'PROFILE'), 'user'), group(
    row({ key: 'name', title: C(285, 'Username'), value: u[1], chevron: true, on: async () => {
      const v = await form(C(284, 'Edit Profile'), [{ name: 'name', label: C(285, 'Username'), value: u[1] }]);
      if (v && v.name.trim()) { await act('user.rename', { id, name: v.name.trim() }); toast(C(320, 'Profile updated')); }
    } }),
    row({ key: 'pic', title: C(286, 'Profile Picture'), image: img(u[3]), chevron: true, on: () => go('general/avatar/' + id) }),
  ), section(C(334, 'USERS'), 'device'), group(
    active ? null : row({ key: 'switch', title: C(292, 'Switch User'), detail: T('switch_hint'), on: () => act('user.activate', { id }) }),
    u[5] === '1' ? null : row({ key: 'def', title: C(279, 'Default User'), detail: C(306, 'Select a default user'), on: () => act('user.default', { id }) }),
    row({ key: 'del', title: C(289, 'Delete User'), detail: active ? C(297, 'Cannot delete the active user') : C(305, 'Profile data only; shared games and apps are kept'),
      disabled: active || all.length < 2, danger: true, arm: { prompt: C(290, 'Press again to delete') },
      on: async () => { await act('user.delete', { id }); toast(C(319, 'User deleted')); go('general'); } }),
  )];
  return { title: u[1], kicker: C(192, 'Profile'), back: 'general', nodes, topics: ['users'] };
}

async function avatarPage(id) {
  const a = await get('avatars', { user: id }, true);
  const items = rows(a.avatar);
  const grid = h('div', { class: 'grid' }, items.map((x) => h('button', { class: 'card', type: 'button', data: { key: 'a:' + x[0] }, title: x[1],
    on: { click: async () => { await act('user.avatar', { id, spec: x[0] }); toast(C(320, 'Profile updated')); go('general/profile/' + id); } } },
  h('img', { src: img(x[2]), alt: x[1], loading: 'lazy' }))));
  grid.style.gridTemplateColumns = 'repeat(auto-fill, minmax(110px, 1fr))';
  $$('.card', grid).forEach((c) => { c.style.aspectRatio = '1'; });
  return {
    title: C(293, 'Profile Pictures'), back: 'general/profile/' + id, topics: [],
    nodes: [dropZone(C(315, 'Custom'), C(294, 'Custom pictures from your profile folder') + ' (PNG, JPG)', (files) => {
      const okFiles = files.filter((x) => /\.(png|jpe?g)$/i.test(x.file.name));
      uploadFiles(okFiles.map((x) => x.file), () => `/userdata/users/${id}/profile/avatars`, { mkdirs: true }).then(() => { forget('avatars'); refresh(); });
    }, { accept: '.png,.jpg,.jpeg', folders: false }), grid],
  };
}

/* ===================================================== CONNECTIVITY */

const SIGNAL = (dbm) => (dbm >= -55 ? C(200, 'Excellent') : dbm >= -67 ? C(201, 'Good') : dbm >= -75 ? C(202, 'Fair') : C(203, 'Weak'));
const WIFI_STATES = { connected: () => C(198, 'Connected'), connecting: () => C(254, 'Connecting to {0}…', ''), disconnected: () => C(159, 'Not connected') };

let scanning = null;
function stopScans() {
  if (scanning === 'wifi') act('wifi.scan', { v: 0 }, { quiet: true }).catch(() => {});
  if (scanning === 'bt') act('bt.discover', { v: 0 }, { quiet: true }).catch(() => {});
  scanning = null;
}
window.addEventListener('hashchange', () => {
  const p = routeOf();
  const want = p[0] === 'connectivity' && p[1] === 'wifi' ? 'wifi' : p[0] === 'connectivity' && p[1] === 'bt' ? 'bt' : null;
  if (want !== scanning) stopScans();
});
window.addEventListener('pagehide', stopScans);

async function wifiConnect(n) {
  if (n.security === 'open' || n.security === 'Open' || n.saved) {
    await act('wifi.connect', { ssid: n.ssid, password: '', hidden: false });
  } else {
    const v = await form(n.ssid, [{ name: 'password', label: C(172, 'Password'), type: 'password' }]);
    if (!v) return;
    if (v.password.length < 8) { toast(C(263, 'Password must contain at least 8 characters'), '', true); return; }
    await act('wifi.connect', { ssid: n.ssid, password: v.password, hidden: false });
  }
  toast(C(254, 'Connecting to {0}…', n.ssid));
}

page('connectivity', 'settings', () => C(5, 'Connectivity'), async (path) => {
  if (path[0] === 'wifi') return wifiPage();
  if (path[0] === 'network') return networkPage(path[1]);
  if (path[0] === 'bt') return btPage();
  const [w, b, r] = await Promise.all([get('wifi').catch(() => null), get('bluetooth').catch(() => null), get('remote')]);
  const addr = r.address || '';
  const nodes = [section(C(156, 'WI-FI'), 'device')];
  if (w) {
    nodes.push(group(
      row({ key: 'wifi', title: C(157, 'Wi-Fi'), toggle: w.enabled, set: (v) => act('wifi.enable', { v }),
        detail: w.state === 'connected' ? w.ssid + ' • ' + (w.ipv4 || '') : w.enabled ? C(159, 'Not connected') : '' }),
      w.enabled && w.state === 'connected' ? row({ key: 'cur', title: w.ssid, icon: 'wifi', detail: C(198, 'Connected') + ' • ' + SIGNAL(w.signal), chevron: true, on: () => go('connectivity/network/' + w.ssid) }) : null,
      w.enabled ? row({ key: 'connect', title: C(9, 'Connect to Wi-Fi'), detail: C(25, 'Choose a network or add a hidden network'), chevron: true, on: () => go('connectivity/wifi') }) : null,
      row({ key: 'saved', title: C(10, 'Saved Networks'), value: w.saved.length ? w.saved.length + C(225, ' saved') : '', chevron: true, disabled: !w.saved.length,
        on: async () => { const v = await choose(C(10, 'Saved Networks'), w.saved.map((n) => ({ value: n.ssid, label: n.ssid, detail: n.current ? C(198, 'Connected') : n.security }))); if (v) go('connectivity/network/' + v); } })));
  } else nodes.push(group(row({ key: 'wifiwait', title: C(206, 'Waiting for Wi-Fi service'), disabled: true })));
  nodes.push(section(C(160, 'BLUETOOTH'), 'device'));
  if (b && b.present) {
    const paired = b.devices.filter((d) => d.paired);
    nodes.push(group(
      row({ key: 'bt', title: C(161, 'Bluetooth'), toggle: b.powered, set: (v) => act('bt.enable', { v }) }),
      b.powered ? paired.map((d) => btRow(d)) : null,
      b.powered ? row({ key: 'btnew', title: C(11, 'Connect New Device'), detail: C(30, 'Choose a nearby Bluetooth device'), chevron: true, on: () => go('connectivity/bt') }) : null));
  } else nodes.push(group(row({ key: 'btwait', title: b ? C(205, 'Unavailable') : C(207, 'Waiting for Bluetooth service'), disabled: true })));
  nodes.push(section(C(727, 'SERVICES'), 'device'));
  nodes.push(group(
    row({ key: 'ssh', title: C(600, 'SSH, SCP and SFTP'), detail: addr ? 'root@' + addr : C(601, 'Sign in as root with the device password'), toggle: on01(r.ssh),
      set: (v) => act('remote.set', { svc: 'ssh', v }, { topics: ['remote'] }) }),
    row({ key: 'smb', title: C(602, 'File Sharing (SMB)'), detail: addr ? '\\\\' + addr + '\\nuubOS' : '', toggle: on01(r.smb),
      set: (v) => act('remote.set', { svc: 'smb', v }, { topics: ['remote'] }) }),
    row({ key: 'web', title: C(603, 'Web Administration'), detail: (addr ? 'https://' + addr : '') + (armedKey === 'web' ? '' : ''), toggle: on01(r.web),
      arm: { prompt: T('web_off') }, on: () => act('remote.set', { svc: 'web', v: 0 }).then(() => showLogin(T('web_disabled'))) }),
    row({ key: 'pw', title: C(172, 'Password'), detail: C(608, 'Press to show'), value: revealed || '••••-••••-••••', on: async () => {
      revealed = revealed ? '' : (await get('password', {}, true)).password;
      setTimeout(() => { revealed = ''; refresh(); }, 30000);
    } }),
    row({ key: 'regen', title: C(606, 'Regenerate Password'), detail: C(607, 'The old password stops working') + ' • ' + T('signs_out'),
      arm: { prompt: C(191, 'Press again to confirm') }, on: async () => { await act('remote.regenerate'); showLogin(T('password_changed')); } })));
  nodes.push(h('div', { class: 'notice', text: C(604, 'Only on networks you trust. Never exposed to the Internet.') }));
  return { title: C(5, 'Connectivity'), nodes, topics: ['wifi', 'bluetooth', 'remote'] };
});

let revealed = '';

function btRow(d) {
  const kind = d.kind === 'audio' || /audio|headset|headphone/i.test(d.kind) ? 'headphones' : /input|gamepad|joystick/i.test(d.kind) ? 'gamepad' : 'bluetooth-b';
  return row({ key: 'bt:' + d.address, title: d.name || d.address, icon: kind,
    detail: [d.connected ? C(198, 'Connected') : d.paired ? C(442, 'Paired') : '', d.battery >= 0 ? C(789, '{0}%', d.battery) : ''].filter(Boolean).join(' • '),
    pill: d.connected ? { text: C(198, 'Connected'), cls: 'on' } : null,
    on: async () => {
      const opts = d.paired
        ? [d.connected ? { value: 'disconnect', label: C(169, 'Disconnect') } : { value: 'connect', label: C(158, 'Connect') }, { value: 'forget', label: C(170, 'Forget Device') }]
        : [{ value: 'pair', label: C(168, 'Pair & Connect') }, { value: 'pin', label: C(171, 'Pair with PIN') }];
      const v = await choose(d.name || d.address, opts);
      if (v === 'pin') {
        const f = await form(C(171, 'Pair with PIN'), [{ name: 'pin', label: C(474, 'PIN') }]);
        if (f) await act('bt.pin', { address: d.address, pin: f.pin });
      } else if (v) await act('bt.' + v, { address: d.address });
    } });
}

async function wifiPage() {
  const w = await get('wifi', {}, true);
  if (scanning !== 'wifi') { scanning = 'wifi'; act('wifi.scan', { v: 1 }, { quiet: true }).catch(() => {}); }
  const nets = [...w.networks].sort((a, b) => b.current - a.current || b.signal - a.signal);
  return {
    title: C(9, 'Connect to Wi-Fi'), back: 'connectivity', topics: ['wifi'],
    nodes: [h('div', { class: 'notice', text: nets.length ? C(164, 'Searching for nearby networks…') : C(23, 'Scanning nearby networks…') }),
      group(nets.map((n) => row({ key: 'n:' + n.ssid, title: n.ssid, icon: 'wifi', detail: [SIGNAL(n.signal), n.security === 'open' ? C(212, 'Open') : n.security, n.saved ? C(199, 'Saved') : ''].filter(Boolean).join(' • '),
        pill: n.current ? { text: C(198, 'Connected'), cls: 'on' } : null,
        on: n.current ? () => go('connectivity/network/' + n.ssid) : async () => {
          const v = n.saved || n.security === 'open' ? 'connect' : await choose(n.ssid, [{ value: 'connect', label: C(158, 'Connect') }, { value: 'wps', label: C(705, 'Connect with WPS') }]);
          if (v === 'wps') { await act('wifi.wps', { ssid: n.ssid }); toast(C(707, 'Press the WPS button on your router…')); } else if (v) await wifiConnect(n);
        } })),
      row({ key: 'hidden', title: C(162, 'Add Hidden Network'), icon: 'plus', on: async () => {
        const v = await form(C(162, 'Add Hidden Network'), [{ name: 'ssid', label: C(739, 'Network name (SSID)') }, { name: 'password', label: C(172, 'Password'), type: 'password' }]);
        if (!v || !v.ssid.trim()) return;
        await act('wifi.connect', { ssid: v.ssid.trim(), password: v.password, hidden: true });
        toast(C(254, 'Connecting to {0}…', v.ssid.trim()));
      } }))],
  };
}

async function networkPage(ssid) {
  const [w, ip] = await Promise.all([get('wifi', {}, true), get('wifi_ip', { ssid }, true).catch(() => ({}))]);
  const n = [...w.saved, ...w.networks].find((x) => x.ssid === ssid) || { ssid };
  const manual = ip.mode === 'manual';
  const editIp = async () => {
    const v = await form(C(175, 'IPv4'), [
      { name: 'mode', label: C(175, 'IPv4'), value: manual ? 'manual' : 'dhcp', options: [{ value: 'dhcp', label: C(122, 'Automatic') + ' (DHCP)' }, { value: 'manual', label: C(176, 'Manual') }] },
      { name: 'address', label: C(177, 'Address'), value: ip.address || ip.runtime_address || '' },
      { name: 'prefix', label: C(178, 'Subnet Mask') + ' (/0–32)', value: String(ip.prefix || ip.runtime_prefix || 24) },
      { name: 'gateway', label: C(179, 'Gateway'), value: ip.gateway || '' },
      { name: 'dns', label: C(180, 'DNS'), value: ip.dns || '' }]);
    if (!v) return;
    await act('wifi.ip', { ssid, ...v });
    toast(C(261, 'Applying network settings…'));
  };
  return {
    title: ssid, kicker: C(157, 'Wi-Fi'), back: 'connectivity', topics: ['wifi'],
    nodes: [group(
      n.current ? row({ key: 'disc', title: C(169, 'Disconnect'), on: () => act('wifi.disconnect') }) : row({ key: 'conn', title: C(158, 'Connect'), on: () => wifiConnect(n) }),
      n.current || n.saved ? null : row({ key: 'wps', title: C(705, 'Connect with WPS'), on: () => act('wifi.wps', { ssid }) }),
      w.wps ? row({ key: 'wpsc', title: C(706, 'Cancel WPS'), detail: C(707, 'Press the WPS button on your router…'), on: () => act('wifi.wpscancel') }) : null,
      n.saved ? row({ key: 'forget', title: C(182, 'Forget Network'), danger: true, arm: { prompt: C(191, 'Press again to confirm') },
        on: async () => { await act('wifi.forget', { ssid }); go('connectivity'); } }) : null),
    section(C(175, 'IPv4'), 'device'),
    group(
      row({ key: 'mode', title: C(175, 'IPv4'), value: manual ? C(176, 'Manual') : C(122, 'Automatic'), chevron: true, disabled: !n.saved, on: editIp }),
      row({ key: 'addr', title: C(177, 'Address'), value: ip.runtime_address || ip.address || '–' }),
      row({ key: 'mask', title: C(178, 'Subnet Mask'), value: ip.runtime_netmask || ip.netmask || '–' }),
      row({ key: 'gw', title: C(179, 'Gateway'), value: ip.runtime_gateway || ip.gateway || '–' }),
      row({ key: 'dns', title: C(180, 'DNS'), value: ip.runtime_dns || ip.dns || '–' }))],
  };
}

async function btPage() {
  const b = await get('bluetooth', {}, true);
  if (scanning !== 'bt') { scanning = 'bt'; act('bt.discover', { v: 1 }, { quiet: true }).catch(() => {}); }
  const found = b.devices.filter((d) => !d.paired).sort((x, y) => y.rssi - x.rssi);
  return {
    title: C(11, 'Connect New Device'), back: 'connectivity', topics: ['bluetooth'],
    nodes: [h('div', { class: 'notice', text: C(29, 'Scanning nearby Bluetooth devices…') }),
      group(found.length ? found.map(btRow) : row({ key: 'none', title: C(166, 'No new Bluetooth devices found'), disabled: true }))],
  };
}

async function syncPage() {
  const s = await get('sync', {}, true);
  const enabled = on01(s.enabled), ready = on01(s.ready);
  const folders = Object.fromEntries(rows(s.folder).map((f) => [f[0], f]));
  const pending = rows(s.pending), devices = rows(s.device), nearby = rows(s.nearby);
  const code = (id) => (id || '').slice(0, 7);
  const label = (d) => d[1] || C(1059, 'Device {0}', code(d[0]));
  const syncing = (f) => ['syncing', 'sync-preparing', 'sync-waiting'].includes(f[2]);
  /* The same status line as the console (syncui.rs). */
  const status = on01(s.failed) ? C(677, 'Syncthing stopped unexpectedly')
    : !enabled ? C(670, 'Keeps your saves in sync with your other devices. It is not a backup.')
    : !ready ? C(1025, 'Starting…')
    : !devices.length ? C(1027, 'No devices yet')
    : Object.values(folders).some((f) => f[1] === '1' && f[2] === 'error') ? C(1084, 'Cannot sync')
    : !devices.some((d) => d[2] === '1') ? C(1026, 'Waiting for your devices')
    : Object.values(folders).some((f) => f[1] === '1' && syncing(f)) || devices.some((d) => d[2] === '1' && +d[3] >= 0 && +d[3] < 100) ? C(323, 'Syncing…')
    : C(489, 'Up to date');
  const nodes = [group(row({ key: 'on', title: C(669, 'Syncthing'), detail: status, toggle: enabled, set: (v) => act('sync.enable', { v }) }))];
  if (enabled) {
    nodes.push(group(
      row({ key: 'me', title: C(1037, 'This Console'), value: s.device_name || '', detail: ready ? C(671, "This Console's ID") : C(1025, 'Starting…') }),
      ready ? h('div', { class: 'notice mono', text: s.device_id }) : null,
      row({ key: 'add', title: C(1028, 'Add a Device'), detail: C(1086, 'Device ID'), icon: 'plus', disabled: !ready, on: async () => {
        const v = await form(C(1028, 'Add a Device'), [{ name: 'id', label: C(1086, 'Device ID') }]);
        if (v && v.id.trim()) await act('sync.add', { device: v.id.trim().toUpperCase() });
      } })));
    nodes.push(section(C(1030, 'DEVICES'), 'user'));
    const list = [
      pending.map((p) => row({ key: 'p:' + p[0], title: label(p), detail: C(1032, 'Wants to sync with you'), icon: 'desktop',
        on: async () => {
          const v = await choose(label(p), [{ value: 'accept', label: C(1033, 'Accept') }, { value: 'ignore', label: C(1034, 'Ignore') }], 'accept');
          if (v) await act(v === 'accept' ? 'sync.add' : 'sync.ignore', { device: p[0] });
        } })),
      devices.map((d) => row({ key: 'd:' + d[0], title: label(d), icon: 'desktop', detail: d[4] || code(d[0]),
        pill: { text: d[2] !== '1' ? C(159, 'Not connected') : +d[3] >= 0 && +d[3] < 100 ? C(1039, '{0}% synced', d[3]) : C(198, 'Connected'), cls: d[2] === '1' ? 'on' : '' },
        arm: { prompt: C(191, 'Press again to confirm') }, on: () => act('sync.remove', { device: d[0] }) })),
      nearby.map((n) => row({ key: 'n:' + n[0], title: C(1059, 'Device {0}', code(n[0])), detail: n[1], icon: 'plus',
        on: () => act('sync.add', { device: n[0] }) })),
    ].flat();
    nodes.push(group(list.length ? list : row({ key: 'none', title: C(1027, 'No devices yet'), disabled: true })));
    nodes.push(section(C(1031, 'WHAT TO SYNC'), 'user'), group(
      [['saves', C(673, 'Saves'), ''], ['states', C(674, 'Save States'), ''], ['screenshots', C(675, 'Screenshots'), ''],
        ['library', C(1089, 'Covers & Metadata'), C(1090, 'Shared by every profile on this console')],
        ['history', C(1091, 'Play Activity'), C(1092, 'Time played and Recently Played, added up across your devices')]].map(([k, title, note]) => row({ key: 'f:' + k,
        title,
        detail: folders[k] && folders[k][1] === '1' ? (syncing(folders[k]) ? C(323, 'Syncing…') : folders[k][2] === 'error' ? C(1084, 'Cannot sync') : note) : note,
        toggle: !!folders[k] && folders[k][1] === '1', set: (v) => act('sync.folder', { kind: k, v }) })),
      row({ key: 'internet', title: C(1035, 'Sync Away from Home'), detail: C(1036, 'Finds your devices over the Internet; your data still goes straight between them'),
        toggle: on01(s.internet), set: (v) => act('sync.internet', { v }) }),
      row({ key: 'relays', title: C(1087, 'Allow Relays'), detail: C(1088, 'If a direct connection is impossible, the encrypted data may pass through public Syncthing servers'),
        toggle: on01(s.internet) && on01(s.relays), disabled: !on01(s.internet), set: (v) => act('sync.relays', { v }) })));
  }
  return { title: C(669, 'Syncthing'), back: 'gaming', nodes, topics: ['sync', 'users'] };
}

/* ================================================ CONTROLLERS & INPUT */

const LIGHT_EFFECTS = [['off', 54, 'Off', 741, 'Lights up only for system signals'], ['solid', 114, 'Solid', 742, 'One steady color'], ['breathe', 115, 'Breathing', 743, 'Slowly fades in and out'],
  ['orbit', 116, 'Orbit', 744, 'A comet of light circles the stick'], ['aurora', 117, 'Aurora', 745, 'Soft shades drift around the ring'], ['spectrum', 118, 'Spectrum', 746, 'Every color, slowly turning'],
  ['battery', 119, 'Battery Level', 747, 'Charge left at a glance, pulses while charging'], ['game', 120, 'Console Colors', 748, 'The color of the system you are playing']];
const LIGHT_COLORS = [['theme', 749, 'Mood Color'], ['3a86ff', 209, 'Blue'], ['00c2e0', 750, 'Cyan'], ['2ee6a6', 751, 'Mint'], ['a6e22e', 752, 'Lime'], ['ffb020', 753, 'Amber'],
  ['ff6a1a', 754, 'Orange'], ['ff2e3e', 755, 'Red'], ['ff3ea5', 756, 'Pink'], ['9b5cff', 757, 'Violet'], ['ffffff', 758, 'White']];
const LIGHT_SIGNALS = [['power', 147, 'Startup & Sleep', 151], ['battery', 100, 'Battery', 152], ['connections', 148, 'Connections', 153], ['games', 149, 'Games', 154], ['tasks', 150, 'Tasks', 155]];
const SIGNAL_DETAILS = { 151: 'Welcome, sleep, wake and power off', 152: 'Low battery, charging and full charge', 153: 'Controllers, headphones, Wi-Fi and USB drives', 154: 'Save states, screenshots and errors', 155: 'Downloads, backups and failures' };

page('controllers', 'settings', () => C(6, 'Controllers & Input'), async (path) => {
  if (path[0] === 'lighting') return lightingPage();
  if (path[0] === 'controller') return controllerPage(path[1]);
  const [c, rm, li] = await Promise.all([get('controllers').catch(() => ({ devices: [], players: [] })), get('rumble').catch(() => null), get('lighting').catch(() => null)]);
  const connected = c.devices.filter((d) => d.connected);
  const nodes = [section(C(734, 'CONTROLLERS'), 'user'), group(
    c.players.map((p) => row({ key: 'p:' + p.player, title: C(220, 'Player ') + p.player, value: p.name ? p.name + (p.available ? '' : C(221, ' • unavailable')) : C(361, 'Not assigned'), chevron: true,
      on: async () => {
        const v = await choose(C(220, 'Player ') + p.player, [{ value: '', label: C(173, 'None') }, ...connected.map((d) => ({ value: d.id, label: d.name, detail: d.kind }))], p.id);
        if (v !== null) await act('ctl.assign', { player: p.player, id: v, name: (connected.find((d) => d.id === v) || {}).name || '' });
      } })).slice(0, Math.max(4, c.players.filter((p) => p.id).length + 1)),
    connected.map((d) => row({ key: 'c:' + d.id, title: d.name, icon: 'gamepad', detail: [d.kind, d.player > 0 ? C(220, 'Player ') + d.player : '', d.battery >= 0 ? C(789, '{0}%', d.battery) : ''].filter(Boolean).join(' • '),
      chevron: true, on: () => go('controllers/controller/' + d.id) }))),
  section(li && li.supported ? C(735, 'RUMBLE & LIGHTING') : C(899, 'RUMBLE'), 'user'), group(
    rm && rm.supported ? row({ key: 'rumble', title: C(17, 'Internal Rumble'), detail: C(37, 'User preference for the internal console rumble'), toggle: rm.enabled, set: (v) => act('rumble.enable', { v }) }) : null,
    rm && rm.supported && rm.enabled ? row({ key: 'rtest', title: C(125, 'Test Rumble'), on: () => act('rumble.test') }) : null,
    li && li.supported ? row({ key: 'light', title: C(18, 'RGB Lighting'), value: C(...(LIGHT_EFFECTS.find((e) => e[0] === li.effect) || [0, 54, 'Off']).slice(1, 3)), chevron: true, on: () => go('controllers/lighting') }) : null)];
  return { title: C(6, 'Controllers & Input'), nodes, topics: ['controllers', 'rumble', 'lighting'] };
});

async function controllerPage(id) {
  const [c, dz] = await Promise.all([get('controllers'), get('deadzones', { id }, true).catch(() => null)]);
  const d = c.devices.find((x) => x.id === id) || { name: id };
  return {
    title: d.name, kicker: C(14, 'Controllers'), back: 'controllers', topics: ['controllers'],
    nodes: [group(
      dz ? row({ key: 'dzl', title: C(123, 'Left Stick Deadzone'), slider: { value: dz.left, min: 0, max: 50, set: (v) => act('ctl.deadzone', { id, stick: 'left', v }, { quiet: true }) } }) : null,
      dz ? row({ key: 'dzr', title: C(124, 'Right Stick Deadzone'), slider: { value: dz.right, min: 0, max: 50, set: (v) => act('ctl.deadzone', { id, stick: 'right', v }, { quiet: true }) } }) : null,
      row({ key: 'reset', title: C(365, 'Restore Default Mapping'), arm: { prompt: C(366, 'Press again to restore defaults') }, on: () => act('ctl.resetmapping', { id }) })),
    h('div', { class: 'notice', text: T('mapping_console') })],
  };
}

async function lightingPage() {
  const li = await get('lighting', {}, true);
  const eff = LIGHT_EFFECTS.find((e) => e[0] === li.effect) || LIGHT_EFFECTS[0];
  const col = LIGHT_COLORS.find((e) => e[0] === li.color);
  const colorful = ['solid', 'breathe', 'orbit'].includes(li.effect);
  return {
    title: C(18, 'RGB Lighting'), back: 'controllers', topics: ['lighting'],
    nodes: [section(C(130, 'LIGHTING'), 'user'), group(
      row({ key: 'eff', title: C(126, 'Effect'), value: C(eff[1], eff[2]), detail: C(eff[3], eff[4]), chevron: true,
        on: () => pickAndAct(C(126, 'Effect'), LIGHT_EFFECTS.map((e) => ({ value: e[0], label: C(e[1], e[2]), detail: C(e[3], e[4]) })), li.effect, 'light.effect') }),
      colorful ? row({ key: 'col', title: C(128, 'Color'), value: col ? C(col[1], col[2]) : '#' + li.color, chevron: true, on: async () => {
        const v = await choose(C(128, 'Color'), [...LIGHT_COLORS.map((c) => ({ value: c[0], label: C(c[1], c[2]) })), { value: '__custom', label: C(759, 'Custom') }], li.color);
        if (v === '__custom') {
          const f = await form(C(759, 'Custom'), [{ name: 'c', label: C(128, 'Color'), type: 'color', value: '#' + (/^[0-9a-f]{6}$/i.test(li.color) ? li.color : '3a86ff') }]);
          if (f) await act('light.color', { v: f.c.replace('#', '').toLowerCase() });
        } else if (v) await act('light.color', { v });
      } }) : null,
      li.effect !== 'off' ? row({ key: 'bri', title: C(127, 'Brightness'), slider: { value: li.brightness, set: (v) => act('light.brightness', { v }, { quiet: true }) } }) : null,
      ['breathe', 'orbit', 'aurora', 'spectrum'].includes(li.effect) ? row({ key: 'spd', title: C(121, 'Speed'), value: C({ slow: 144, normal: 145, fast: 146 }[li.speed] || 145, li.speed), chevron: true,
        on: () => pickAndAct(C(121, 'Speed'), [['slow', 144, 'Slow'], ['normal', 145, 'Normal'], ['fast', 146, 'Fast']].map((s) => ({ value: s[0], label: C(s[1], s[2]) })), li.speed, 'light.speed') }) : null),
    section(C(129, 'SYSTEM SIGNALS'), 'user'),
    group(LIGHT_SIGNALS.map((s, i) => row({ key: 'sig:' + s[0], title: C(s[1], s[2]), detail: C(s[3], SIGNAL_DETAILS[s[3]]), toggle: !!(li.signals & (1 << i)),
      set: (v) => act('light.signal', { category: s[0], v }) })))],
  };
}

/* =========================================================== GAMING */

const HOTKEYS = [['save_state', 417, 'Save State'], ['load_state', 418, 'Load State'], ['slot_next', 779, 'Next Slot'], ['slot_prev', 780, 'Previous Slot'],
  ['fast_forward', 781, 'Fast Forward'], ['quit', 422, 'Quit Game'], ['screenshot', 592, 'Screenshot']];
const CONTROLS = [['menu_confirm', 337, 'Face South'], ['menu_back', 338, 'Face East'], ['face_north', 339, 'Face North'], ['face_west', 340, 'Face West'],
  ['menu_up', 341, 'D-Pad Up'], ['menu_down', 342, 'D-Pad Down'], ['menu_left', 343, 'D-Pad Left'], ['menu_right', 344, 'D-Pad Right'],
  ['l1', 345, 'L1'], ['r1', 346, 'R1'], ['l2', 347, 'L2'], ['r2', 348, 'R2'], ['l3', 349, 'L3'], ['r3', 350, 'R3'], ['settings', 351, 'Start'], ['select', 352, 'Select']];
const OVERLAY = [['fps', 783, 'Frame Rate'], ['cpu', 0, 'CPU'], ['gpu', 0, 'GPU'], ['ram', 0, 'RAM'], ['temp', 784, 'Temperature'], ['power', 785, 'Power Draw'], ['battery', 786, 'Battery'], ['clock', 787, 'Clock']];
const SOURCES = { screenscraper: ['ScreenScraper', 793], thegamesdb: ['TheGamesDB', 794], libretro: ['libretro', 795] };
const SOURCE_DETAILS = { 793: 'Covers, descriptions and ratings in your language. A free ScreenScraper account gives more requests.', 794: 'Covers and descriptions in English. A free private API key gives more requests.', 795: 'Covers only. No account needed.' };
const controlName = (c) => { const x = CONTROLS.find((k) => k[0] === c); return x ? C(x[1], x[2]) : c; };

page('gaming', 'settings', () => C(702, 'Gaming'), async (path) => {
  if (path[0] === 'ra') return raPage();
  if (path[0] === 'hotkeys') return hotkeysPage();
  if (path[0] === 'overlay') return overlayPage();
  if (path[0] === 'sync') return syncPage();
  const [emu, ra, ss, lib, jobs, sync] = await Promise.all([get('emu_settings'), get('ra'), get('scraper'), get('library'), get('jobs'),
    get('sync').catch(() => ({}))]);
  const set = (key, value) => act('emu.set', { key, value }, { topics: ['emu_settings'] });
  const busy = rows(jobs.job).some((j) => j[1] === 'scrape-bulk' && (j[2] === 'running' || j[2] === 'queued'));
  const avail = on01(ss.available);
  const src = SOURCES[ss.provider] || SOURCES.screenscraper;
  const scanning = lib.scanning === '1';
  const nodes = [section(C(703, 'GAMING'), 'user'), group(
    row({ key: 'ra', title: C(584, 'RetroAchievements'), value: ra.user || C(566, 'Not signed in'), chevron: true, on: () => go('gaming/ra') }),
    row({ key: 'boot', title: C(767, 'Console Boot Screens'), detail: C(768, "Shows the console's start-up logo when its BIOS is available"), toggle: on01(emu.boot_screens), set: (v) => set('boot_screens', v ? '1' : '0') }),
    row({ key: 'slots', title: C(769, 'Save State Slots'), value: emu.slots, chevron: true, on: async () => {
      const v = await choose(C(769, 'Save State Slots'), ['3', '5', '10', '20', '50', '99'].map((n) => ({ value: n, label: n })), emu.slots);
      if (v) await set('slots', v);
    } }),
    row({ key: 'rot', title: C(770, 'Rotate Save Slots'), detail: C(771, 'Each save goes to the next slot, then starts again from slot 0'), toggle: on01(emu.slot_rotation), set: (v) => set('slot_rotation', v ? '1' : '0') }),
    row({ key: 'hk', title: C(772, 'Game Hotkeys'), chevron: true, on: () => go('gaming/hotkeys') }),
    row({ key: 'ov', title: C(774, 'Performance Overlay'), value: on01(emu.overlay) ? C(222, 'On') : C(54, 'Off'), chevron: true, on: () => go('gaming/overlay') }),
    row({ key: 'bez', title: C(775, 'Bezels'), detail: C(776, 'Frames the game when it does not fill the screen'), toggle: on01(emu.bezels), set: (v) => set('bezels', v ? '1' : '0') }),
    row({ key: 'crt', title: C(777, 'CRT Effect'), detail: C(778, 'Scanlines for consoles played on TVs'), toggle: on01(emu.crt), set: (v) => set('crt', v ? '1' : '0') })),
  section(C(704, 'LIBRARY'), 'device'), group(
    row({ key: 'scan', title: C(804, 'Scan for New Games'), detail: C(805, 'Finds games copied to the SD card since the last scan'), busy: scanning,
      value: scanning ? C(410, 'Searching for games…') : '', disabled: scanning, on: () => act('lib.scan') }),
    row({ key: 'src', title: C(792, 'Metadata Source'), value: src[0], detail: C(src[1], SOURCE_DETAILS[src[1]]), chevron: true,
      on: () => pickAndAct(C(792, 'Metadata Source'), Object.entries(SOURCES).map(([value, s]) => ({ value, label: s[0], detail: C(s[1], SOURCE_DETAILS[s[1]]) })), ss.provider, 'scraper.provider') }),
    ss.provider === 'screenscraper' || !ss.provider ? (ss.user
      ? row({ key: 'ssout', title: C(565, 'ScreenScraper Account'), value: ss.user, detail: C(567, 'Sign Out'), arm: { prompt: C(191, 'Press again to confirm') }, on: () => act('scraper.signout') })
      : row({ key: 'ssin', title: C(565, 'ScreenScraper Account'), value: C(566, 'Not signed in'), detail: C(803, 'Optional: more requests per day'), on: async () => {
        const v = await form(C(565, 'ScreenScraper Account'), [{ name: 'name', label: C(285, 'Username') }, { name: 'password', label: C(172, 'Password'), type: 'password' }]);
        if (v && v.name.trim()) { await act('scraper.account', v); toast(C(588, 'Signed in')); }
      } })) : null,
    ss.provider === 'thegamesdb' ? row({ key: 'tgdb', title: C(796, 'TheGamesDB API Key'), value: ss.apikey ? '•••• ' + ss.apikey : C(797, 'Not set'), detail: C(803, 'Optional: more requests per day'),
      on: async () => { const v = await form(C(796, 'TheGamesDB API Key'), [{ name: 'key', label: C(796, 'TheGamesDB API Key') }]); if (v) await act('scraper.apikey', v); } }) : null,
    row({ key: 'bulk', title: C(568, 'Get Metadata for All Games'), busy, value: busy ? C(571, 'Getting metadata') : '',
      detail: avail ? C(569, 'Uses the Internet only when you ask. Games without metadata keep their file name.') : C(570, 'The metadata service is not available'),
      disabled: !avail || busy, on: () => act('scraper.bulk', { scope: 'all' }, { topics: ['jobs'] }) }),
    row({ key: 'fill', title: C(798, 'Fill Missing Data'), detail: C(799, 'Adds only what is missing, such as covers or descriptions. What is already there is kept.'), disabled: !avail || busy,
      on: async () => {
        const v = await choose(C(798, 'Fill Missing Data'), [['missing', 870, 'Every game with missing data'], ['missing:3600', 871, 'Not tried in the last hour'], ['missing:86400', 872, 'Not tried in the last day'],
          ['missing:604800', 873, 'Not tried in the last week'], ['missing:2592000', 874, 'Not tried in the last month']].map((o) => ({ value: o[0], label: C(o[1], o[2]) })));
        if (v) await act('scraper.bulk', { scope: v }, { topics: ['jobs'] });
      } }),
    ss.provider !== 'libretro' ? row({ key: 'lang', title: C(868, 'Metadata in Your Language'), detail: C(869, 'Gets descriptions and genres in your language; the languages already downloaded stay saved'),
      disabled: !avail || busy, on: () => act('scraper.bulk', { scope: 'lang' }, { topics: ['jobs'] }) }) : null,
    row({ key: 'bios', title: C(576, 'BIOS Files'), detail: C(581, 'Copy BIOS files to the bios folder on the SD card. nuubOS never includes them.'), chevron: true, on: () => go('bios') })),
    /* SYNCTHING (user request 2026-10-12: here, not in Connectivity). */
    section(C(1094, 'SYNCTHING'), 'user'), group(
      row({ key: 'sync', title: C(669, 'Syncthing'), value: on01(sync.enabled) ? C(222, 'On') : C(54, 'Off'),
        detail: C(670, 'Keeps your saves in sync with your other devices. It is not a backup.'), chevron: true, on: () => go('gaming/sync') }))];
  return { title: C(702, 'Gaming'), nodes, topics: ['emu_settings', 'ra', 'scraper', 'library', 'jobs', 'sync'] };
});

async function raPage() {
  const ra = await get('ra', {}, true);
  return {
    title: C(584, 'RetroAchievements'), back: 'gaming', topics: ['ra'],
    nodes: [group(ra.user
      ? [row({ key: 'user', title: C(584, 'RetroAchievements'), value: ra.user, detail: C(588, 'Signed in') }),
        row({ key: 'en', title: C(112, 'Enabled'), toggle: on01(ra.enabled), set: (v) => act('ra.set', { key: 'enabled', v }) }),
        row({ key: 'hc', title: C(585, 'Hardcore Mode'), detail: C(586, 'No save states or fast-forward; achievements count double'), toggle: on01(ra.hardcore), set: async (v) => {
          /* Turning it on first explains what it takes away. */
          if (v && !(await form(C(907, 'Turn On Hardcore Mode?'), [], { text: C(908, 'Achievements are earned as on the original console, with no help. While Hardcore Mode is on, save states and fast-forward are off: every game starts from the beginning and only the game\'s own saves are kept. RetroArch also blocks cheats, rewind and slow motion. You can turn it off at any time.'), ok: C(909, 'Turn On') }))) return;
          await act('ra.set', { key: 'hardcore', v });
        } }),
        row({ key: 'out', title: C(567, 'Sign Out'), arm: { prompt: C(191, 'Press again to confirm') }, on: () => act('ra.logout') })]
      : row({ key: 'in', title: C(587, 'Sign In'), detail: C(591, 'Your password is used once and never stored'), on: async () => {
        const v = await form(C(584, 'RetroAchievements'), [{ name: 'name', label: C(285, 'Username') }, { name: 'password', label: C(172, 'Password'), type: 'password' }], { text: C(591, 'Your password is used once and never stored'), ok: C(587, 'Sign In') });
        if (!v || !v.name.trim()) return;
        toast(C(590, 'Signing in…'));
        try { await act('ra.login', v, { quiet: true }); toast(C(588, 'Signed in')); } catch (e) { toast(C(589, 'Sign-in failed'), '', true); }
      } }))],
  };
}

async function hotkeysPage() {
  const emu = await get('emu_settings', {}, true);
  return {
    title: C(772, 'Game Hotkeys'), back: 'gaming', topics: ['emu_settings'],
    nodes: [h('div', { class: 'notice', text: C(773, 'Hold Hotkey and press a button') }), group(HOTKEYS.map(([k, i, f]) => row({ key: k, title: C(i, f), value: controlName(emu['hotkey_' + k]), chevron: true,
      on: async () => {
        const v = await choose(C(i, f), CONTROLS.map((c) => ({ value: c[0], label: C(c[1], c[2]) })), emu['hotkey_' + k]);
        if (v) await act('emu.set', { key: 'hotkey_' + k, value: v }, { topics: ['emu_settings'] });
      } })))],
  };
}

async function overlayPage() {
  const emu = await get('emu_settings', {}, true);
  const shown = (emu.overlay_items || '').split(',').filter(Boolean);
  const on = on01(emu.overlay);
  const setItems = (field, v) => {
    const next = OVERLAY.map((o) => o[0]).filter((f) => (f === field ? v : shown.includes(f)));
    return act('emu.set', { key: 'overlay_items', value: next.join(',') }, { topics: ['emu_settings'] });
  };
  return {
    title: C(774, 'Performance Overlay'), back: 'gaming', topics: ['emu_settings'],
    nodes: [group(row({ key: 'on', title: C(782, 'Show Overlay'), toggle: on, set: (v) => act('emu.set', { key: 'overlay', value: v ? '1' : '0' }, { topics: ['emu_settings'] }) }),
      OVERLAY.map(([f, i, fb]) => row({ key: 'o:' + f, title: i ? C(i, fb) : fb, toggle: shown.includes(f), disabled: !on, set: (v) => setItems(f, v) })))],
  };
}

/* =================================================== DISPLAY & AUDIO */

page('display', 'settings', () => C(7, 'Display & Audio'), async () => {
  const [d, a] = await Promise.all([get('display'), get('audio')]);
  const outputs = { auto: C(122, 'Automatic'), analog: C(332, 'Analog Audio'), bluetooth: C(331, 'Bluetooth Audio'), hdmi: 'HDMI' };
  const sel = { speaker: C(240, 'Speaker'), headphones: C(330, 'Headphones'), bluetooth: C(331, 'Bluetooth Audio'), hdmi: 'HDMI' };
  const vol = a['volume.' + a.selected];
  const nodes = [section(C(728, 'DISPLAY'), 'device'), group(
    d.hdmi ? row({ key: 'bri', title: C(127, 'Brightness'), detail: C(132, 'Internal display inactive'), disabled: true })
      : row({ key: 'bri', title: C(127, 'Brightness'), slider: { value: Number(d.brightness) || 60, min: 1, max: 100, set: (v) => act('display.brightness', { v }, { quiet: true }) } }),
    d.temperature_supported ? row({ key: 'temp', title: C(143, 'Color Temperature'), slider: { value: Number(d.temperature) || 6500, min: 3500, max: 7500, step: 250, text: (v) => v + C(224, ' K'),
      set: (v) => act('display.temperature', { v }, { quiet: true }) } }) : null),
  section(C(729, 'AUDIO'), 'device'), group(
    row({ key: 'out', title: C(133, 'Audio Output'), value: (a.mode === 'auto' ? C(122, 'Automatic') + ' • ' : '') + (sel[a.selected] || a.selected), chevron: true, on: () => {
      const opts = [{ value: 'auto', label: outputs.auto }];
      if (a['available.bluetooth'] === '1') opts.push({ value: 'bluetooth', label: outputs.bluetooth });
      opts.push({ value: 'analog', label: outputs.analog });
      if (a['available.hdmi'] === '1') opts.push({ value: 'hdmi', label: outputs.hdmi });
      return pickAndAct(C(133, 'Audio Output'), opts, a.mode, 'audio.output');
    } }),
    vol === 'unsupported' ? row({ key: 'vol', title: C(134, 'Volume'), detail: C(273, 'Volume controlled by TV / receiver'), disabled: true })
      : row({ key: 'vol', title: C(134, 'Volume'), slider: { value: Number(vol) || 0, set: (v) => act('audio.volume', { v }, { quiet: true }) } }),
    row({ key: 'apps', title: C(135, 'Applications'), slider: { value: Number(a['volume.applications']) || 0, set: (v) => act('audio.apps', { v }, { quiet: true }) } }),
    row({ key: 'music', title: C(139, 'Home Music'), slider: { value: Number(a['volume.home_music']) || 0, set: (v) => act('audio.music', { v }, { quiet: true }) } }),
    on01(a['home_music.playing']) ? row({ key: 'skip', title: C(329, 'Skip Track'), detail: a['home_music.track'] || '', on: () => act('audio.musicplay', { v: 'NEXT' }) }) : null,
    row({ key: 'test', title: C(140, 'Test Sound'), value: T('on_console'), on: () => act('audio.test', { v: 'START' }) })),
  section(C(730, 'SOUNDS'), 'user'), group(
    row({ key: 'sys', title: C(136, 'System Sounds'), slider: { value: Number(a['volume.system']) || 0, set: (v) => act('audio.system', { v }, { quiet: true }) } }),
    row({ key: 'nav', title: C(137, 'Navigation Sounds'), toggle: on01(a['system.navigation_enabled']), set: (v) => act('audio.nav', { v }) }),
    row({ key: 'notif', title: C(851, 'Notification Sounds'), toggle: a['system.notify_enabled'] === undefined ? true : on01(a['system.notify_enabled']), set: (v) => act('audio.notify', { v }) }),
    row({ key: 'pow', title: C(138, 'Power Sounds'), toggle: on01(a['system.power_enabled']), set: (v) => act('audio.power', { v }) }))];
  return { title: C(7, 'Display & Audio'), nodes, topics: ['display', 'audio'] };
});

/* ============================================================ POWER */

const BATTERY_HEALTH = { good: [820, 'Good'], overheat: [821, 'Overheating'], cold: [822, 'Too cold'], dead: [823, 'Worn out'] };

page('power', 'settings', () => C(813, 'Power'), async () => {
  const [s, st, c, b] = await Promise.all([get('system'), get('status'), get('controllers').catch(() => ({ devices: [] })), get('bluetooth').catch(() => ({ devices: [] }))]);
  const health = BATTERY_HEALTH[(st.BATTERY_HEALTH || '').toLowerCase()];
  const bstate = s['battery.state'];
  const accessories = [
    ...c.devices.filter((d) => d.connected && !d.builtin && d.battery >= 0).map((d) => ({ name: d.name, battery: d.battery })),
    ...(b.devices || []).filter((d) => d.connected && d.battery >= 0 && !/input|gamepad/i.test(d.kind)).map((d) => ({ name: d.name, battery: d.battery }))];
  const nodes = [section(C(826, 'ENERGY SAVING'), 'device'), group(
    row({ key: 'prof', title: C(50, 'Performance Profile'), value: s['profile.requested'] === 'battery-saver' ? C(51, 'Battery Saver') : C(52, 'Auto'),
      detail: s['profile.effective'] && s['profile.effective'] !== s['profile.requested'] ? C(214, 'Effective: ') + (s['profile.effective'] === 'battery-saver' ? C(51, 'Battery Saver') : C(52, 'Auto')) : '',
      chevron: true, on: () => pickAndAct(C(50, 'Performance Profile'), [{ value: 'auto', label: C(52, 'Auto') }, { value: 'battery-saver', label: C(51, 'Battery Saver') }], s['profile.requested'], 'system.profile') }),
    row({ key: 'auto', title: C(53, 'Automatic Battery Saver'), value: Number(s['auto_battery.threshold']) ? '≤ ' + s['auto_battery.threshold'] + '%' : C(54, 'Off'), chevron: true,
      on: () => pickAndAct(C(53, 'Automatic Battery Saver'), [0, 10, 15, 20, 25, 30, 40, 50].map((v) => ({ value: String(v), label: v ? '≤ ' + v + '%' : C(54, 'Off') })), s['auto_battery.threshold'], 'system.autobattery') }),
    row({ key: 'sleep', title: C(56, 'Sleep After'), value: Number(s.sleep_after_min) ? s.sleep_after_min + ' min' : C(54, 'Off'), chevron: true,
      on: () => pickAndAct(C(56, 'Sleep After'), [0, 1, 2, 5, 10, 15, 30, 45, 60, 120].map((v) => ({ value: String(v), label: v ? v + ' min' : C(54, 'Off') })), s.sleep_after_min, 'system.sleep') })),
  section(C(814, 'BATTERY'), 'device'), group(
    row({ key: 'charge', title: C(816, 'Charge'), value: s['battery.percent'] + '%', detail: bstate === 'charging' ? C(382, 'Charging') : bstate === 'full' ? C(383, 'Fully charged') : bstate === 'not-charging' ? C(819, 'Not charging') : C(818, 'Discharging') }),
    row({ key: 'health', title: C(817, 'Battery Health'), value: health ? C(health[0], health[1]) : st.BATTERY_HEALTH ? C(824, 'Check the battery') : C(211, 'Unknown') }),
    ...(s['charge_limit.supported'] === '1' ? [row({ key: 'climit', title: C(900, 'Charge Limit'), value: Number(s.charge_limit) < 100 ? s.charge_limit + '%' : C(54, 'Off'), chevron: true,
      on: () => pickAndAct(C(900, 'Charge Limit'), [80, 85, 90, 95, 100].map((v) => ({ value: String(v), label: v < 100 ? v + '%' : C(54, 'Off') })), s.charge_limit, 'system.chargelimit') })] : [])),
  section(C(815, 'ACCESSORY BATTERIES'), 'device'), group(accessories.length
    ? accessories.map((x, i) => row({ key: 'acc' + i, title: x.name, value: x.battery + '%' }))
    : row({ key: 'noacc', title: C(825, 'No accessories connected'), disabled: true }))];
  return { title: C(813, 'Power'), nodes, topics: ['system', 'status', 'controllers', 'bluetooth'] };
});

/* =========================================================== SYSTEM */

page('system', 'settings', () => C(8, 'System'), async (path) => {
  if (path[0] === 'storage') return storagePage();
  if (path[0] === 'update') return updatePage();
  if (path[0] === 'info') return infoPage();
  if (path[0] === 'recovery') return recoveryPage();
  const [s, jobs] = await Promise.all([get('system'), get('jobs')]);
  const report = rows(jobs.job).filter((j) => j[1] === 'support-bundle').pop();
  const nodes = [section(C(732, 'STORAGE & SOFTWARE'), 'device'), group(
    row({ key: 'storage', title: C(19, 'Storage'), value: ['ready', 'readonly'].includes(s['storage.sd2.state']) ? C(912, 'SD1') + ' • ' + C(64, 'SD2') : C(912, 'SD1'), chevron: true, on: () => go('system/storage') }),
    row({ key: 'update', title: C(651, 'Software Update'), value: 'nuubOS ' + (s['nuubos.version'] || ''), chevron: true, on: () => go('system/update') }),
    row({ key: 'info', title: C(20, 'System Information'), chevron: true, on: () => go('system/info') })),
  section(C(733, 'TROUBLESHOOTING'), 'device'), group(
    row({ key: 'diag', title: C(500, 'Save Diagnostic Report'), detail: C(504, 'Useful for getting help. It contains no games, saves, passwords or Wi-Fi keys and is saved in the diagnostics folder.'),
      busy: report && report[2] === 'running', on: async () => { await act('diag.bundle', {}, { topics: ['jobs'] }); toast(C(501, 'Saving diagnostic report…')); } }),
    report && report[2] === 'succeeded' && report[6] ? row({ key: 'diagdl', title: T('download_report'), icon: 'download', detail: report[6].split('/').pop(),
      on: () => { location.href = '/api/download?' + new URLSearchParams({ p: report[6] }); } }) : null,
    row({ key: 'rec', title: C(499, 'Recovery'), chevron: true, on: () => go('system/recovery') }))];
  return { title: C(8, 'System'), nodes, topics: ['system', 'jobs'] };
});

async function storagePage() {
  const s = await get('system', {}, true);
  const n = (k) => Number(s[k]) || 0;
  const space = (free, total) => C(914, '{0} free of {1}').replace('{0}', sizeText(free)).replace('{1}', sizeText(total));
  const st = s['storage.sd2.state'] || '';
  const slot = s['storage.sd2.slot'] === '1';
  const mounted = st === 'ready' || st === 'readonly';
  const sd2 = {
    ready: [space(n('storage.sd2.free'), n('storage.sd2.total')), s['storage.sd2.label'] || C(915, 'Games card')],
    readonly: [C(918, 'Read-only'), C(919, 'The card has errors: games can be played, nothing can be written to it')],
    checking: [C(917, 'Checking…'), ''],
    unreadable: [C(920, 'Unreadable'), C(921, 'No supported file system (exFAT, FAT32, ext4)')],
    ejected: [C(922, 'Ejected'), C(627, 'Safe to remove')],
    ignored: [C(978, 'Not in use'), C(980, 'nuubOS leaves this card alone until you choose to use it')],
  }[st] || [C(916, 'No card'), ''];
  const items = (s['storage.backup.items'] || '').split(',');
  const itemRows = [['saves', 947, 'Saves'], ['states', 948, 'Save States'], ['library', 949, 'Game Library'],
    ['screenshots', 951, 'Screenshots & Recordings'], ['bios', 576, 'BIOS Files'], ['covers', 953, 'Covers & Metadata']]
    .map(([k, id, en]) => row({ key: 'it-' + k, title: C(id, en), toggle: items.includes(k),
      set: (v) => act('storage.backup.item', { item: k, v: v ? '1' : '0' }) }));
  const last = n('storage.backup.last');
  /* The console backup: one, on the destination the user chose. */
  const dest = s['storage.backup.dest'] || 'sd1';
  const available = s['storage.backup.available'] === '1';
  const destName = (d) => (d === 'sd2' ? C(64, 'SD2') : d === 'usb' ? C(1016, 'USB Drive') : C(912, 'SD1'));
  const missing = dest === 'sd2' ? C(935, 'Insert an SD2 card to back up') : dest === 'usb' ? C(1021, 'Connect a USB drive to back up') : '';
  return {
    title: C(19, 'Storage'), back: 'system', topics: ['system', 'jobs'],
    nodes: [group(
      row({ key: 'sd1', title: C(912, 'SD1'), value: space(n('storage.userdata_free'), n('storage.userdata_total')), detail: C(913, 'System and saves') }),
      slot ? row({ key: 'sd2', title: C(64, 'SD2'), value: sd2[0], detail: sd2[1] }) : null,
      slot && mounted ? row({ key: 'eject', title: C(924, 'Eject SD2'), on: () => act('storage.eject') }) : null,
      slot && st === 'ejected' ? row({ key: 'use', title: C(925, 'Use SD2 Again'), on: () => act('storage.use') }) : null,
      slot && st === 'ignored' ? row({ key: 'use', title: C(979, 'Use SD2'), on: () => act('storage.use') }) : null,
      slot && st === 'ready' ? row({ key: 'prep', title: C(926, 'Create Game Folders'), detail: C(927, 'Adds a folder for each system. Nothing is deleted.'),
        on: () => act('storage.prepare', {}, { topics: ['jobs'] }) }) : null,
      slot && (mounted || st === 'unreadable' || st === 'ignored') && s['storage.sd2.cid'] ? row({ key: 'fmt', title: C(928, 'Format SD2'), detail: C(929, 'Erases everything on the card'), danger: true,
        arm: { prompt: C(930, 'Press again • everything on SD2 will be erased') }, on: () => act('storage.format', { cid: s['storage.sd2.cid'] }, { topics: ['jobs'] }) }) : null),
    section(C(1024, 'BACKUP'), 'device'),
    group(
      row({ key: 'auto', title: C(69, 'Automatic Backup'), toggle: s['storage.backup.enabled'] === '1',
        detail: available ? (last ? C(933, 'Last backup: {0}', new Date(last * 1000).toLocaleString()) : C(934, 'Never backed up')) : missing,
        set: (v) => act('storage.backup.enabled', { v: v ? '1' : '0' }) }),
      row({ key: 'dest', title: C(1015, 'Destination'), value: destName(dest), detail: dest === 'sd1' ? C(1022, 'Lost if SD1 is formatted or replaced') : '', chevron: true,
        on: async () => {
          const v = await choose(C(1015, 'Destination'), [...(slot ? ['sd2'] : []), 'sd1', 'usb'].map((d) => ({ value: d, label: destName(d) })), dest);
          if (v !== null && v !== dest) await act('storage.backup.dest', { v });
        } }),
      s['storage.backup.card'] !== 'conflict' ? row({ key: 'bk', title: C(88, 'Back Up Now'), disabled: !available, on: () => act('storage.backup', {}, { topics: ['jobs'] }) })
        : row({ key: 'conflict', title: C(942, 'Backup of a previous SD1'), detail: C(943, 'Restore it, or keep it aside and start a new backup') })),
    section(C(936, 'Backup Content'), 'device'),
    group(row({ key: 'prof', title: C(945, 'Profiles & Settings'), detail: C(946, 'Always included. Passwords and accounts are never copied.'), disabled: true }), ...itemRows),
    section(C(625, 'Removable Media'), 'device'),
    group(row({ key: 'media', title: C(611, 'Network Shares') + ' / ' + C(625, 'Removable Media'), chevron: true, on: () => go('files/:shares') }))],
  };
}

/* Release notes in a scrollable sheet, only when asked for. */
function showNotes(title, lines) {
  openModal(h('div', { class: 'sheet', role: 'dialog', 'aria-label': title }, h('h3', { text: title }),
    h('div', { class: 'opts notes' }, ...lines.map((l) => h('p', { text: '• ' + l }))),
    button(C(239, 'Close'), () => modalClose(null))), () => {});
}

async function updatePage() {
  const u = await get('update', {}, true);
  const available = on01(u.available), downloaded = on01(u.downloaded);
  const channels = [{ value: 'stable', label: C(986, 'Stable') }, { value: 'beta', label: C(987, 'Beta') }, { value: 'alpha', label: C(988, 'Alpha') }];
  const channel = channels.find((c) => c.value === u.channel) || channels[0];
  /* Release notes: one manifest line, "\\n" = new line, "\\\\" = backslash. */
  const notes = available ? (u.notes || '').replace(/\\(n|\\)/g, (m, c) => (c === 'n' ? '\n' : '\\'))
    .split('\n').map((l) => l.trim().replace(/^[-*•]\s*/, '')).filter((l) => l) : [];
  return {
    title: C(651, 'Software Update'), back: 'system', topics: ['update', 'jobs'],
    nodes: [group(
      row({ key: 'ver', title: C(485, 'Version'), value: u.build ? u.current + ' • ' + u.build : u.current }),
      u.provider !== 'available' ? row({ key: 'na', title: C(655, 'Updates are not available in this build'), disabled: true }) : [
        row({ key: 'chan', title: C(985, 'Update Channel'), value: channel.label, chevron: true,
          detail: channel.value !== 'stable' ? C(989, 'Beta and Alpha get new versions first and may have bugs') : '',
          on: () => pickAndAct(C(985, 'Update Channel'), channels, channel.value, 'update.channel', 'v', ['update']) }),
        row({ key: 'auto', title: C(992, 'Check Automatically'), detail: C(993, 'Once a day, when connected to Wi-Fi'), toggle: on01(u.auto),
          set: (v) => act('update.auto', { v: v ? '1' : '0' }, { topics: ['update'] }) }),
        row({ key: 'check', title: C(487, 'Check for Updates'), value: available ? C(652, 'nuubOS {0} available', u.version) : '',
          on: async () => { toast(C(488, 'Checking…')); await act('update.check', {}, { topics: ['update'] }); const n = await get('update', {}, true); if (!on01(n.available)) toast(C(489, 'Up to date')); } }),
        available && !downloaded ? row({ key: 'dl', title: C(653, 'Download'), on: () => act('update.download', {}, { topics: ['jobs'] }) }) : null,
        available && downloaded && u.apply_battery === 'low' ? row({ key: 'inst', title: C(654, 'Install and Restart'), detail: C(1012, 'Charge the battery above 30% to install'),
          on: () => toast(C(1012, 'Charge the battery above 30% to install')) }) : null,
        available && downloaded && u.apply_battery !== 'low' ? row({ key: 'inst', title: C(654, 'Install and Restart'), detail: C(657, 'Your games, saves and settings are kept'), arm: { prompt: C(191, 'Press again to confirm') },
          on: async () => { await act('update.install', {}, { topics: ['jobs'] }); toast(C(984, 'Installing update'), T('console_restarts'), false, 0); } }) : null,
        notes.length ? row({ key: 'notes', title: C(1005, "What's New"), chevron: true, on: () => showNotes(C(1005, "What's New") + ' • nuubOS ' + u.version, notes) }) : null])],
  };
}

async function infoPage() {
  const s = await get('system', {}, true);
  const up = Number(s.uptime_sec) || 0;
  const info = [
    [C(98, 'Device'), s['device.model']], ['nuubOS', (s['nuubos.version'] || '') + (s['nuubos.build'] ? ' • ' + s['nuubos.build'] : '')], [C(99, 'Kernel'), s.kernel],
    [C(100, 'Battery'), s['battery.percent'] + '%'], [C(101, 'CPU'), (Number(s['cpu.freq_khz']) / 1000).toFixed(0) + ' MHz • ' + (Number(s['temp.cpu_millic']) / 1000).toFixed(1) + ' °C'],
    [C(102, 'GPU'), (Number(s['gpu.freq_hz']) / 1e6).toFixed(0) + ' MHz • ' + (Number(s['temp.gpu_millic']) / 1000).toFixed(1) + ' °C'], [C(103, 'Network'), s.network],
    [C(104, 'RAM'), sizeText((Number(s['mem.total_kib']) - Number(s['mem.available_kib'])) * 1024) + ' / ' + sizeText(Number(s['mem.total_kib']) * 1024)],
    [C(105, 'Uptime'), Math.floor(up / 3600) + ' h ' + Math.floor((up % 3600) / 60) + ' min']];
  return { title: C(20, 'System Information'), back: 'system', topics: ['system'], nodes: [group(info.map(([k, v]) => row({ key: k, title: k, value: v || '–' })))] };
}

async function recoveryPage() {
  const r = await get('recovery', {}, true).catch(() => ({}));
  return {
    title: C(499, 'Recovery'), back: 'system', topics: [],
    nodes: [group(
      r['safe-mode'] === '1' ? row({ key: 'safe', title: C(518, 'Safe Mode'), detail: C(519, 'The interface closed unexpectedly several times.') }) : null,
      row({ key: 'repair', title: C(505, 'Repair Settings'), detail: C(512, 'Finds and fixes damaged settings files. Games and saves are not touched.'),
        on: async () => { await act('rec.repair'); toast(C(506, 'Check complete')); } }),
      row({ key: 'ruser', title: C(508, 'Reset User Profile'), detail: C(513, "Resets the current user's preferences, controls and app settings. Saves are kept. The console restarts."),
        danger: true, arm: { prompt: C(516, 'Press again to confirm • the console restarts') }, on: async () => { await act('rec.resetuser'); toast(C(509, 'Profile reset'), T('console_restarts')); } }),
      row({ key: 'factory', title: C(510, 'Factory Reset'), detail: C(514, 'Erases users, settings, Wi-Fi networks and Bluetooth pairings, then starts the initial setup. Games and BIOS are kept.'),
        danger: true, arm: { prompt: C(516, 'Press again to confirm • the console restarts') }, on: async () => { await act('rec.factory', { v: 'settings' }); showLogin(T('console_restarts')); } }),
      row({ key: 'full', title: C(511, 'Full Factory Reset'), detail: C(515, "Like Factory Reset, and also erases every user's saves, states, screenshots and app data. Games and BIOS are kept."),
        danger: true, arm: { prompt: C(516, 'Press again to confirm • the console restarts') }, on: async () => {
          const ok = await form(C(511, 'Full Factory Reset'), [], { text: C(515, "Like Factory Reset, and also erases every user's saves, states, screenshots and app data. Games and BIOS are kept."), ok: C(511, 'Full Factory Reset') });
          if (ok) { await act('rec.factory', { v: 'userdata' }); showLogin(T('console_restarts')); }
        } }))],
  };
}
