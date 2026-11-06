// The browser UI: the same WebSocket and skin files the desktop app uses (docs/PROTOCOL.md, docs/SKIN.md), served by
// the addin itself. Lists MPC's plugin instances, draws the selected one's skin page on a canvas (skin.js lays it out)
// or a card per parameter, sends set messages for clicks and drags, and shows the transport and the project snapshot.
import { Skin, hit, press, css, fontFor, stripFrame } from './skin.js';

const $ = id => document.getElementById(id);
const DRAG_PX = 200;          // generic card: screen pixels of drag per unit of value
const FETCHES = 2;            // skin files in flight at once: the addin serves max_clients connections, the socket is one
const SEND_MS = 25;           // a drag sends at most this often per parameter

const S = {
  ws: null, up: false, hello: null, retry: 0, pingAt: 0, rtt: null,
  insts: new Map(),           // id -> {plugin, params: Map i -> param, options: Map i -> Map text -> value}
  sel: null, view: new Map(), // uid -> page index, or -1 for the parameter cards
  skins: new Map(),           // uid -> {state, skin, images, done, total, error}
  transport: null, drag: null, layout: null,
};

// ---- connection ----
function connect() {
  const ws = new WebSocket(`${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/ws`);
  S.ws = ws;
  conn('connecting');
  ws.onopen = () => { S.up = true; S.retry = 0; conn('up'); };
  ws.onmessage = e => { let m; try { m = JSON.parse(e.data); } catch { return; } onMessage(m); };
  ws.onclose = () => {
    if (S.ws !== ws) return;
    S.up = false;
    conn('down');
    setTimeout(connect, Math.min(5000, 500 * 2 ** S.retry++));
  };
}
const send = m => { if (S.up && S.ws.readyState === 1) S.ws.send(JSON.stringify(m)); };
setInterval(() => { if (S.up) { S.pingAt = performance.now(); send({ t: 'ping' }); } }, 5000);

function conn(state) {
  const el = $('conn');
  el.className = 'conn' + (state === 'up' ? ' up' : state === 'down' ? ' down' : '');
  el.querySelector('span').textContent =
    state === 'up' ? (S.rtt === null ? 'connected' : `connected · ${S.rtt} ms`) : state === 'down' ? 'reconnecting' : 'connecting';
}

function onMessage(m) {
  switch (m.t) {
    case 'hello': {
      S.hello = m;
      const d = m.device || {};
      $('device').textContent = [d.model, d.mpc && `MPC ${d.mpc}`, m.addin && `addin ${m.addin}`].filter(Boolean).join(' · ');
      const midiOff = !m.midi || m.midi.error;
      for (const b of document.querySelectorAll('#transport [data-cmd]')) {
        b.disabled = !!midiOff;
        b.title = midiOff ? `No MIDI port: ${(m.midi && m.midi.error) || 'off'}` : b.dataset.cmd[0].toUpperCase() + b.dataset.cmd.slice(1);
      }
      $('transport').hidden = false;
      S.rackApplying = false;
      send({ t: 'rack' });
      break;
    }
    case 'plugins':
      S.insts.clear();
      for (const p of m.plugins || []) addInst(p);
      if (!S.insts.has(S.sel)) S.sel = null;
      renderList();
      select(S.sel ?? fromHash() ?? firstId());
      break;
    case 'plugin_added':
      addInst(m.plugin);
      renderList();
      if (S.sel === null) select(m.plugin.id);
      break;
    case 'plugin_removed':
      S.insts.delete(m.id);
      renderList();
      if (S.sel === m.id) select(firstId());
      break;
    case 'values': {
      const inst = S.insts.get(m.id);
      if (!inst) break;
      for (const [i, v, text] of m.v || []) {
        const p = inst.params.get(i);
        if (!p) continue;
        const held = S.drag && S.drag.id === m.id && S.drag.param === i;   // the pointer owns it while it drags
        if (!held) p.value = v;
        if (text !== null && text !== undefined) p.text = text;
        learn(inst, i, p.value, p.text);
      }
      if (m.id === S.sel) redraw();
      break;
    }
    case 'transport': S.transport = m; renderTransport(); break;
    case 'project': renderProject(m); break;
    case 'rack': S.rack = m; $('rack-btn').hidden = !m.available; renderRack(); break;
    case 'rack_result': rackResult(m); break;
    case 'rack_live_result': S.rackMsg = `Not switched: ${m.error}`; renderRack(); break;
    case 'pong': S.rtt = Math.round(performance.now() - S.pingAt); conn('up'); break;
    case 'error': tip(`The addin says: ${m.msg}`, 5000); if ($('project-body').dataset.wait) renderProjectError(m.msg); break;
  }
}

function addInst(p) {
  const params = new Map();
  for (const q of p.params || []) params.set(q.i, { ...q });
  const inst = { plugin: p, params, options: new Map() };
  for (const q of params.values()) learn(inst, q.i, q.value, q.text);
  S.insts.set(p.id, inst);
}
const firstId = () => (S.insts.size ? S.insts.keys().next().value : null);

// #<id>/<page> (or /all): which instance and page, so a reload comes back to it while MPC keeps running (ids are
// MPC's for the life of its process).
function fromHash() {
  const m = /^#(\d+)(?:\/(all|\d+))?$/.exec(location.hash);
  const inst = m && S.insts.get(+m[1]);
  if (!inst) return null;
  if (m[2]) S.view.set(inst.plugin.uid, m[2] === 'all' ? -1 : +m[2]);
  return +m[1];
}
function toHash(id, page) {
  const h = id === null ? '' : `#${id}/${page < 0 ? 'all' : page}`;
  if (location.hash !== h) history.replaceState(null, '', h || location.pathname);
}

// A display text that reads as a number, with or without a unit.
function isNumeric(text) {
  const m = String(text).trim().match(/^[0-9+\-.,]+/);
  return !!m && Number.isFinite(Number(m[0].replace(/,/g, '.')));
}
// Remembers a non-numeric display text and the value that showed it: the generic card offers them as chips.
function learn(inst, i, value, text) {
  if (!text || isNumeric(text)) return;
  let seen = inst.options.get(i);
  if (!seen) inst.options.set(i, (seen = new Map()));
  if (seen.size < 64 || seen.has(text)) seen.set(text, value);
}

const source = inst => ({
  value: i => inst.params.get(i)?.value ?? 0,
  name: i => inst.params.get(i)?.name ?? '',
  text: i => inst.params.get(i)?.text ?? '',
});

// ---- set, coalesced ----
const pending = new Map();
let flushTimer = 0;
function setParam(id, i, value) {
  value = Math.min(1, Math.max(0, value));
  const p = S.insts.get(id)?.params.get(i);
  if (p) p.value = value;            // shown until the next poll confirms it
  pending.set(`${id}:${i}`, { id, i, value });
  if (!flushTimer) flushTimer = setTimeout(flush, SEND_MS);
  redraw();
}
function flush() {
  flushTimer = 0;
  for (const m of pending.values()) send({ t: 'set', id: m.id, i: m.i, value: m.value });
  pending.clear();
}

// ---- plugin list ----
function renderList() {
  const ul = $('plugins');
  ul.textContent = '';
  for (const [id, inst] of S.insts) {
    const p = inst.plugin;
    const li = document.createElement('li');
    li.className = id === S.sel ? 'sel' : '';
    const sk = S.skins.get(p.uid);
    const dot = document.createElement('i');
    dot.className = 'dot' + (p.skin ? ' ' + (sk ? sk.state : '') : '');
    dot.title = !p.skin ? 'No skin: parameter cards' : sk ? `Skin ${sk.state}` : 'Skin';
    const name = document.createElement('div');
    name.className = 'name';
    name.textContent = p.name || p.product || `Plugin ${id}`;
    const vendor = document.createElement('span');
    vendor.className = 'vendor';
    vendor.textContent = p.vendor || '';
    name.append(vendor);
    const badge = document.createElement('span');
    badge.className = 'badge ' + (p.synth ? 'synth' : 'fx');
    badge.textContent = p.synth ? 'SYNTH' : 'FX';
    li.append(dot, name, badge);
    li.onclick = () => { select(id); document.body.classList.remove('menu-open'); };
    ul.append(li);
  }
  $('no-plugins').hidden = S.insts.size > 0;
}

function select(id) {
  S.sel = id;
  S.drag = null;
  send({ t: 'subscribe', ids: id === null ? [] : [id] });
  renderList();
  const inst = S.insts.get(id);
  const title = $('title');
  title.textContent = '';
  if (inst) {
    title.append(inst.plugin.name || inst.plugin.product || '');
    const small = document.createElement('small');
    small.textContent = [inst.plugin.vendor, inst.plugin.synth ? 'instrument' : 'effect'].filter(Boolean).join(' · ');
    title.append(small);
    if (inst.plugin.skin && !S.skins.has(inst.plugin.uid)) loadSkin(inst.plugin.uid, `/skin/${id}`);
  }
  S.genericFor = null;
  render();
}

// ---- skin files ----
const queue = [];
let active = 0;
const sleep = ms => new Promise(r => setTimeout(r, ms));
function getFile(url) {
  return new Promise((resolve, reject) => { queue.push({ url, resolve, reject }); pump(); });
}
function pump() {
  while (active < FETCHES && queue.length) {
    const job = queue.shift();
    active++;
    fetchRetry(job.url).then(job.resolve, job.reject).finally(() => { active--; pump(); });
  }
}
// "busy" (503: every connection the addin allows is taken) and network errors are tried again; 404 is final.
async function fetchRetry(url) {
  for (let a = 0; ; a++) {
    let status = 0;
    try {
      const r = await fetch(url, { cache: 'no-cache' });
      if (r.ok) return r;
      status = r.status;
    } catch { /* network: retry */ }
    if (status === 404 || a >= 6) throw new Error(`${status || 'network error'}: ${decodeURI(url)}`);
    await sleep(250 * (a + 1));
  }
}
const fileUrl = (prefix, file) => `${prefix}/${file.split('/').map(encodeURIComponent).join('/')}`;
async function decodeImage(blob) {
  if (window.createImageBitmap) return createImageBitmap(blob);
  const img = new Image();
  img.src = URL.createObjectURL(blob);
  await img.decode();
  return img;
}

async function loadSkin(uid, prefix) {
  const s = { state: 'loading', skin: null, images: new Map(), done: 0, total: 0, error: '' };
  S.skins.set(uid, s);
  renderList();
  try {
    const skin = new Skin(await (await getFile(`${prefix}/TUI.json`)).json());
    // Imported definition files: a plugin's skin folder only serves what is inside it, so one outside it (../) is
    // skipped and its components draw as generic widgets, as in the desktop app.
    const todo = [...skin.imports], seen = new Set();
    while (todo.length) {
      const f = todo.pop();
      if (seen.has(f) || seen.size > 16 || f.startsWith('../')) continue;
      seen.add(f);
      try { todo.push(...skin.addLibrary(f, await (await getFile(fileUrl(prefix, f))).json())); }
      catch (e) { console.warn(`skin ${uid}: ${f}: ${e.message}`); }
    }
    s.skin = skin;
    s.state = 'ready';
    const files = skin.imageFiles().filter(f => !f.startsWith('../'));
    s.total = files.length;
    renderList();
    redraw();
    // Pages draw as images arrive; a missing one stays a placeholder outline.
    await Promise.all(files.map(async f => {
      try { s.images.set(f, await decodeImage(await (await getFile(fileUrl(prefix, f))).blob())); }
      catch (e) { console.warn(`skin ${uid}: ${e.message}`); }
      s.done++;
      if (S.insts.get(S.sel)?.plugin.uid === uid) redraw();
    }));
  } catch (e) {
    s.state = 'failed';
    s.error = e.message || String(e);
    renderList();
  }
  if (S.insts.get(S.sel)?.plugin.uid === uid) render();
}

// ---- the stage ----
let raf = 0;
function redraw() { if (!raf) raf = requestAnimationFrame(() => { raf = 0; render(); }); }

function render() {
  const inst = S.insts.get(S.sel);
  const canvas = $('skin'), generic = $('generic'), status = $('status'), tabs = $('tabs');
  status.textContent = '';
  if (!inst) {
    canvas.hidden = generic.hidden = true;
    tabs.textContent = '';
    status.textContent = S.up ? 'No plugin selected.' : 'Connecting to the addin…';
    return;
  }
  const sk = inst.plugin.skin ? S.skins.get(inst.plugin.uid) : null;
  const pages = sk && sk.state === 'ready' ? sk.skin.pages : [];
  let page = S.view.get(inst.plugin.uid) ?? 0;
  if (page >= pages.length) page = sk && sk.state === 'loading' && page >= 0 ? page : pages.length ? 0 : -1;
  toHash(inst.plugin.id, page);
  if (page >= pages.length) {   // the skin is still loading
    canvas.hidden = generic.hidden = true;
    tabs.textContent = '';
    tabs.dataset.key = '';
    status.textContent = 'Loading the skin…';
    return;
  }
  renderTabs(inst, pages, page);
  if (page < 0) {
    canvas.hidden = true;
    generic.hidden = false;
    renderGeneric(inst);
    if (sk && sk.state === 'failed') tip(`Skin failed (${sk.error}): showing the parameters`, 0);
    return;
  }
  generic.hidden = true;
  canvas.hidden = false;
  drawPage(inst, sk, page);
  if (sk.done < sk.total) { tip(`Loading skin images ${sk.done}/${sk.total}…`, 0); S.loadingTip = true; }
  else if (S.loadingTip) { S.loadingTip = false; tip('', 0); }
  if (!S.drag && sk.done >= sk.total) hoverTip();
}

function renderTabs(inst, pages, page) {
  const tabs = $('tabs');
  const key = `${inst.plugin.id}|${pages.length}|${page}`;
  if (tabs.dataset.key === key) return;
  tabs.dataset.key = key;
  tabs.textContent = '';
  if (!pages.length) return;
  const add = (label, name, index) => {
    const b = document.createElement('button');
    b.className = index === page ? 'sel' : '';
    if (label) { const f = document.createElement('b'); f.textContent = label; b.append(f); }
    b.append(name);
    b.onclick = () => { S.view.set(inst.plugin.uid, index); S.genericFor = null; render(); };
    tabs.append(b);
  };
  pages.forEach((p, i) => add(`F${p.fnKey + 1}${p.subIndex ? '.' + p.subIndex : ''}`, p.name || `Page ${i + 1}`, i));
  add('', 'All parameters', -1);
}

// ---- drawing a skin page ----
const canvas = $('skin');
const ctx = canvas.getContext('2d');
const view = { scale: 1 };

function fit(size) {
  const panel = $('panel');
  const scale = Math.max(0.05, Math.min(panel.clientWidth / size.w, panel.clientHeight / size.h));
  const w = Math.floor(size.w * scale), h = Math.floor(size.h * scale), dpr = window.devicePixelRatio || 1;
  if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
  }
  canvas.style.width = `${w}px`;
  canvas.style.height = `${h}px`;
  view.scale = scale;
  view.dpr = dpr;
}

function drawPage(inst, sk, page) {
  const info = sk.skin.pages[page];
  fit(info.size);
  const layout = sk.skin.layout(page, source(inst));
  S.layout = { inst, layout };
  const k = view.scale * view.dpr;
  ctx.setTransform(k, 0, 0, k, 0, 0);
  ctx.fillStyle = '#000';
  ctx.fillRect(0, 0, info.size.w, info.size.h);
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = 'high';
  for (const it of layout.items) drawItem(it, sk, inst);
}

function placeholder(r) {
  ctx.strokeStyle = '#3d2a6b';
  ctx.lineWidth = 1;
  ctx.strokeRect(r.x + 0.5, r.y + 0.5, Math.max(0, r.w - 1), Math.max(0, r.h - 1));
}

function drawItem(it, sk, inst) {
  const r = it.rect;
  switch (it.t) {
    case 'image':
    case 'button': {
      const img = sk.images.get(it.file);
      if (img) ctx.drawImage(img, r.x, r.y, r.w, r.h); else placeholder(r);
      break;
    }
    case 'knob': {
      const img = sk.images.get(it.file);
      if (!img) { placeholder(r); break; }
      const fw = img.width, f = stripFrame(img.width, img.height, it.frame, it.frames);
      // A frame smaller than its rect draws at its own size from the rect's top left (AIR's slider track).
      const d = fw < r.w && f.h < r.h ? { x: r.x, y: r.y, w: fw, h: f.h } : r;
      ctx.drawImage(img, 0, f.y, fw, f.h, d.x, d.y, d.w, d.h);
      break;
    }
    case 'slider': {
      const img = sk.images.get(it.thumb);
      if (!img) { placeholder(r); break; }
      const v = Math.min(1, Math.max(0, it.value));
      const x = it.vertical ? r.x + r.w / 2 - img.width / 2 : r.x + Math.max(0, r.w - img.width) * v;
      const y = it.vertical ? r.y + r.h - img.height - Math.max(0, r.h - img.height) * v : r.y + r.h / 2 - img.height / 2;
      ctx.drawImage(img, x, y, img.width, img.height);
      break;
    }
    case 'fill': ctx.fillStyle = css(it.colour); ctx.fillRect(r.x, r.y, r.w, r.h); break;
    case 'arrow':
      ctx.fillStyle = css(it.colour);
      ctx.beginPath();
      ctx.moveTo(r.x, r.y);
      ctx.lineTo(r.x + r.w, r.y);
      ctx.lineTo(r.x + r.w / 2, r.y + r.h);
      ctx.closePath();
      ctx.fill();
      break;
    case 'label': drawLabel(r, it.text, it.style); break;
    case 'meter': {
      ctx.fillStyle = '#251744';
      ctx.fillRect(r.x, r.y, r.w, r.h);
      const v = Math.min(1, Math.max(0, it.value));
      ctx.fillStyle = '#3dffa8';
      ctx.fillRect(r.x, r.y + r.h * (1 - v), r.w, r.h * v);
      break;
    }
    case 'generic': {
      ctx.fillStyle = '#251744';
      ctx.fillRect(r.x, r.y, r.w, r.h);
      ctx.strokeStyle = '#5b468f';
      ctx.strokeRect(r.x + 0.5, r.y + 0.5, r.w - 1, r.h - 1);
      const p = it.param === null ? null : inst.params.get(it.param);
      ctx.fillStyle = '#f1e9ff';
      ctx.font = '600 14px "Titillium Web"';
      ctx.textAlign = 'center';
      ctx.textBaseline = 'middle';
      const cx = r.x + r.w / 2, cy = r.y + r.h / 2;
      ctx.fillText(p ? p.name : it.name, cx, p ? cy - 9 : cy, r.w);
      if (p) ctx.fillText(p.text, cx, cy + 9, r.w);
      ctx.textBaseline = 'alphabetic';
      break;
    }
  }
}

// A label as JUCE places it: the text anchored by its justification inside the box, the font sized by ascent plus
// descent, clipped to the box (with a little room above and below, as the app allows).
function drawLabel(r, text, style) {
  if (!text) return;
  const f = fontFor(style.font);
  ctx.font = `${f.weight} ${f.em}px "${f.family}", sans-serif`;
  const m = ctx.measureText(text);
  const asc = m.fontBoundingBoxAscent ?? style.font.height * 0.745;
  const desc = m.fontBoundingBoxDescent ?? style.font.height * 0.255;
  const j = style.justification;
  let x = r.x;
  ctx.textAlign = 'left';
  if (j.h === 'centre') { x = r.x + r.w / 2; ctx.textAlign = 'center'; }
  else if (j.h === 'right') { x = r.x + r.w; ctx.textAlign = 'right'; }
  const y = j.v === 'top' ? r.y + asc : j.v === 'bottom' ? r.y + r.h - desc : r.y + r.h / 2 + (asc - desc) / 2;
  ctx.save();
  ctx.beginPath();
  ctx.rect(r.x, r.y - 3, r.w, r.h + 6);
  ctx.clip();
  ctx.fillStyle = css(style.colour);
  ctx.textBaseline = 'alphabetic';
  ctx.fillText(text, x, y);
  ctx.restore();
}

// ---- pointer on the skin ----
const toPage = e => {
  const b = canvas.getBoundingClientRect();
  return [(e.clientX - b.left) / view.scale, (e.clientY - b.top) / view.scale];
};
let hoverAt = null;
canvas.addEventListener('pointerdown', e => {
  if (!S.layout) return;
  const { inst, layout } = S.layout;
  const [x, y] = toPage(e);
  const c = hit(layout.controls, x, y);
  const a = c && press(c, source(inst));
  if (!a) return;
  e.preventDefault();
  if ('set' in a) { setParam(inst.plugin.id, a.param, a.set); return; }
  S.drag = { id: inst.plugin.id, param: a.param, start: a.drag.start, y0: e.clientY,
             ppu: Math.max(1, a.drag.height * view.scale), last: a.drag.start };
  canvas.setPointerCapture(e.pointerId);
  paramTip(inst, a.param);
});
canvas.addEventListener('pointermove', e => {
  const d = S.drag;
  if (d) {
    const v = Math.min(1, Math.max(0, d.start + (d.y0 - e.clientY) / d.ppu));
    if (Math.abs(v - d.last) > 0.0005) { d.last = v; setParam(d.id, d.param, v); }
    const inst = S.insts.get(d.id);
    if (inst) paramTip(inst, d.param);
    return;
  }
  hoverAt = toPage(e);
  hoverTip();
});
const endDrag = () => { if (S.drag) { S.drag = null; flush(); } };
canvas.addEventListener('pointerup', endDrag);
canvas.addEventListener('pointercancel', endDrag);
canvas.addEventListener('pointerleave', () => { hoverAt = null; if (!S.drag) tip('', 0); });

function hoverTip() {
  if (!S.layout || !hoverAt) return;
  const c = hit(S.layout.layout.controls, hoverAt[0], hoverAt[1]);
  canvas.style.cursor = c && c.param !== null && c.gesture.kind !== 'inert' && c.gesture.kind !== 'select'
    ? (c.gesture.kind === 'drag' ? 'ns-resize' : 'pointer') : 'default';
  if (c && c.param !== null) paramTip(S.layout.inst, c.param); else tip('', 0);
}
function paramTip(inst, i) {
  const p = inst.params.get(i);
  if (p) tip(`${p.name} = ${p.text}${p.label ? ' ' + p.label : ''}  (${p.value.toFixed(3)})`, 0);
}
let tipTimer = 0;
function tip(text, ms) {
  clearTimeout(tipTimer);
  $('tip').textContent = text;
  if (ms) tipTimer = setTimeout(() => { $('tip').textContent = ''; }, ms);
}

// ---- generic panel: a card per parameter ----
function renderGeneric(inst) {
  const box = $('generic');
  if (S.genericFor !== inst.plugin.id) {
    S.genericFor = inst.plugin.id;
    box.textContent = '';
    for (const p of inst.params.values()) {
      if (p.name.endsWith('__open')) continue;
      box.append(card(inst, p.i));
    }
  }
  for (const el of box.children) updateCard(inst, el);
}

function card(inst, i) {
  const el = document.createElement('div');
  el.className = 'card';
  el.dataset.i = i;
  el.innerHTML = '<div class="pname"></div><svg viewBox="0 0 64 56"><path class="track" fill="none" stroke="#3d2a6b" ' +
    'stroke-width="4" stroke-linecap="round"/><path class="val" fill="none" stroke="#ff2fa0" stroke-width="4" ' +
    'stroke-linecap="round"/><line class="tip" stroke="#f1e9ff" stroke-width="2" stroke-linecap="round"/>' +
    '<circle cx="32" cy="30" r="3" fill="#f1e9ff"/></svg><div class="opt"></div><div class="ptext"></div>' +
    '<div class="chips"></div><div class="raw"></div>';
  el.querySelector('.track').setAttribute('d', arc(0, 1));
  let drag = null;
  el.addEventListener('pointerdown', e => {
    if (e.target.closest('.chips button')) return;
    const p = inst.params.get(i);
    drag = { y: e.clientY, start: p ? p.value : 0 };
    S.drag = { id: inst.plugin.id, param: i };
    el.setPointerCapture(e.pointerId);
  });
  el.addEventListener('pointermove', e => {
    if (!drag) return;
    setParam(inst.plugin.id, i, drag.start + (drag.y - e.clientY) / DRAG_PX);
  });
  const end = () => { if (drag) { drag = null; S.drag = null; flush(); } };
  el.addEventListener('pointerup', end);
  el.addEventListener('pointercancel', end);
  return el;
}

// A 270-degree arc from the bottom left, the first `v` of it.
function arc(from, to) {
  const a0 = (135 + 270 * from) * Math.PI / 180, a1 = (135 + 270 * to) * Math.PI / 180, R = 24;
  const pt = a => `${(32 + R * Math.cos(a)).toFixed(2)} ${(30 + R * Math.sin(a)).toFixed(2)}`;
  return `M ${pt(a0)} A ${R} ${R} 0 ${a1 - a0 > Math.PI ? 1 : 0} 1 ${pt(a1)}`;
}

function updateCard(inst, el) {
  const i = +el.dataset.i;
  const p = inst.params.get(i);
  if (!p) return;
  const optionLike = p.text && !isNumeric(p.text);
  el.querySelector('.pname').textContent = p.name;
  el.querySelector('.pname').title = `parameter ${i}`;
  el.querySelector('svg').style.display = optionLike ? 'none' : '';
  const opt = el.querySelector('.opt');
  opt.style.display = optionLike ? '' : 'none';
  el.querySelector('.ptext').style.display = optionLike ? 'none' : '';
  const v = Math.min(1, Math.max(0, p.value));
  if (optionLike) opt.textContent = p.text;
  else {
    el.querySelector('.val').setAttribute('d', v > 0.001 ? arc(0, v) : '');
    const a = (135 + 270 * v) * Math.PI / 180;
    const t = el.querySelector('.tip');
    t.setAttribute('x1', 32); t.setAttribute('y1', 30);
    t.setAttribute('x2', (32 + 18 * Math.cos(a)).toFixed(2)); t.setAttribute('y2', (30 + 18 * Math.sin(a)).toFixed(2));
    el.querySelector('.ptext').textContent = p.label ? `${p.text} ${p.label}` : p.text;
  }
  const chips = el.querySelector('.chips');
  const opts = optionLike ? [...(inst.options.get(i) || new Map())].sort((a, b) => a[1] - b[1]).slice(0, 6) : [];
  const key = opts.map(o => o[0]).join('\u0001') + '|' + p.value;
  if (chips.dataset.key !== key) {
    chips.dataset.key = key;
    chips.textContent = '';
    if (opts.length > 1)
      for (const [text, value] of opts) {
        const b = document.createElement('button');
        b.textContent = text;
        b.className = Math.abs(value - p.value) < 1e-4 ? 'cur' : '';
        b.onclick = () => setParam(inst.plugin.id, i, value);
        chips.append(b);
      }
  }
  el.querySelector('.raw').textContent = p.value.toFixed(3);
}

// ---- transport ----
for (const b of document.querySelectorAll('#transport [data-cmd]')) b.onclick = () => send({ t: 'transport', cmd: b.dataset.cmd });
function renderTransport() {
  const t = S.transport;
  $('pos').textContent = `${t.bar ?? 1}.${t.beat ?? 1}.${String(t.tick ?? 0).padStart(3, '0')}`;
  $('tempo').textContent = typeof t.tempo === 'number' ? `${t.tempo.toFixed(1)} bpm` : '--- bpm';
  $('tstate').textContent = (t.recording ? 'recording' : t.playing ? 'playing' : 'stopped') + (t.source ? ` · ${t.source}` : '');
  document.querySelector('[data-cmd="play"]').classList.toggle('on', !!t.playing && !t.recording);
  document.querySelector('[data-cmd="record"]').classList.toggle('on', !!t.recording);
}

// ---- project snapshot ----
$('project-btn').onclick = () => {
  $('project').hidden = false;
  const body = $('project-body');
  body.dataset.wait = '1';
  if (!body.firstChild) body.innerHTML = '<p class="note">Reading the project…</p>';
  send({ t: 'project' });
};
$('project-close').onclick = () => { $('project').hidden = true; };
function renderProjectError(msg) {
  const body = $('project-body');
  delete body.dataset.wait;
  body.textContent = '';
  const p = document.createElement('p');
  p.className = 'note';
  p.textContent = `No project: ${msg}`;
  body.append(p);
}
function renderProject(m) {
  const body = $('project-body');
  delete body.dataset.wait;
  body.textContent = '';
  const dl = document.createElement('dl');
  const row = (k, v) => {
    if (v === undefined || v === null || v === '') return;
    const dt = document.createElement('dt'), dd = document.createElement('dd');
    dt.textContent = k; dd.textContent = v;
    dl.append(dt, dd);
  };
  row('Name', m.name);
  row('Tempo', typeof m.tempo === 'number' ? `${m.tempo.toFixed(2)} bpm` : '');
  row('Key', m.key);
  if (m.sequence) row('Sequence', `${m.sequence.index}: ${m.sequence.name || ''} (${m.sequence.bars} bars)`);
  row('Saved', m.mtime ? new Date(m.mtime * 1000).toLocaleString() : '');
  body.append(dl);
  const table = document.createElement('table');
  for (const t of m.tracks || []) {
    const tr = table.insertRow();
    const n = tr.insertCell(); n.className = 'n'; n.textContent = t.index;
    const c = tr.insertCell();
    const sw = document.createElement('span');
    sw.className = 'sw';
    sw.style.background = `#${(t.colour >>> 0 & 0xffffff).toString(16).padStart(6, '0')}`;
    c.append(sw, t.name || '(unnamed)');
    const sub = document.createElement('div');
    sub.className = 'note';
    sub.textContent = [t.type, t.plugin && t.plugin.name + (t.plugin.preset ? ` · ${t.plugin.preset}` : ''),
                       typeof t.volume === 'number' ? `vol ${Math.round(t.volume * 100)}%` : ''].filter(Boolean).join(' · ');
    c.append(sub);
    const f = tr.insertCell(); f.className = 'flags';
    for (const [on, label] of [[t.mute || t.track_mute, 'M'], [t.solo, 'S'], [t.record_arm, 'REC']])
      if (on) { const s = document.createElement('span'); s.textContent = label; f.append(s); }
  }
  body.append(table);
  const note = document.createElement('p');
  note.className = 'note';
  note.textContent = 'From the project file as MPC last saved it, not the live state.';
  body.append(note);
}

// ---- the MPC Sample rack: add / remove / turn off plugins (the rack addin edits the project and restarts MPC; a plugin
// MPC is running turns on and off at once, until MPC restarts, and Apply writes those OFFs into the project) ----
// Edits queue here, shown as pending, and go out together on Apply (two clicks: it restarts MPC).
S.rack = null; S.edits = []; S.rackMsg = ''; S.rackApplying = false; S.rackConfirm = 0;
$('rack-btn').onclick = () => {
  $('project').hidden = true;
  $('rack').hidden = false;
  send({ t: 'rack' });
  renderRack();
};
$('rack-close').onclick = () => { $('rack').hidden = true; };
$('project-btn').addEventListener('click', () => { $('rack').hidden = true; });

const findEdit = (op, slot, index, name) => S.edits.findIndex(e => e.op === op && e.slot === slot && (op === 'add' ? e.plugin === name : e.index === index && e.name === name));
const usedNames = () => {
  const used = new Set();
  for (const s of S.rack.slots) for (const f of s.fx) if (f.vst && findEdit('remove', s.id, f.index, f.name) < 0) used.add(f.name);
  for (const e of S.edits) if (e.op === 'add') used.add(e.plugin);
  return used;
};
const slotCount = s => s.fx.filter(f => findEdit('remove', s.id, f.index, f.name) < 0).length +
                       S.edits.filter(e => e.op === 'add' && e.slot === s.id).length;
function toggleEdit(op, slot, f, value) {
  const i = findEdit(op, slot, f.index, f.name);
  if (i >= 0) S.edits.splice(i, 1);
  else {
    if (op === 'remove') { const j = findEdit('enable', slot, f.index, f.name); if (j >= 0) S.edits.splice(j, 1); }
    S.edits.push({ op, slot, index: f.index, name: f.name, value });
  }
  S.rackConfirm = 0; S.rackMsg = '';
  renderRack();
}
// the edits Apply sends: the queued ones, and an OFF for each plugin that is off live and not already in them
const liveOffs = () => {
  const out = [];
  for (const s of S.rack.slots) for (const f of s.fx)
    if (f.vst && f.live === 'off' && findEdit('remove', s.id, f.index, f.name) < 0 && findEdit('enable', s.id, f.index, f.name) < 0)
      out.push({ op: 'enable', slot: s.id, index: f.index, name: f.name, value: false });
  return out;
};
function renderRack() {
  const body = $('rack-body');
  body.textContent = '';
  const el = (tag, cls, text) => { const e = document.createElement(tag); if (cls) e.className = cls; if (text !== undefined) e.textContent = text; return e; };
  const r = S.rack;
  if (!r || !r.available) { body.append(el('p', 'note', r ? `No rack here: ${r.reason}.` : 'Reading the rack…')); return; }
  if (!r.ok) { body.append(el('p', 'note', `The project can't be read: ${r.error}`)); return; }
  if (S.rackApplying || r.applying) { body.append(el('p', 'rack-msg', 'MPC is restarting with your changes. This page reconnects by itself.')); return; }
  body.append(el('p', 'note', `Project: ${r.project || '(untitled)'}. MPC's own effects stay as they are; one of each plugin per project.`));
  const used = usedNames();
  for (const s of r.slots) {
    const box = el('div', 'rack-slot');
    box.append(el('h3', '', s.label));
    for (const f of s.fx) {
      const gone = findEdit('remove', s.id, f.index, f.name) >= 0;
      const en = findEdit('enable', s.id, f.index, f.name);
      const live = f.vst && f.live && en < 0 && !gone;   // MPC is running it: on/off now
      const on = en >= 0 ? S.edits[en].value : live ? f.live === 'on' : f.enabled;
      const row = el('div', 'rack-fx' + (f.vst ? '' : ' own') + (gone ? ' gone' : ''));
      row.append(el('span', 'nm', f.name));
      if (!f.vst) row.append(el('span', 'tag', 'MPC'));
      else {
        if (en >= 0 && !gone) row.append(el('span', 'tag', 'pending'));
        if (live && !on) row.append(el('span', 'tag', 'off until restart'));
        const t = el('button', on ? 'on' : '', on ? 'ON' : 'OFF');
        t.disabled = gone;
        t.title = live ? 'Turn this plugin on or off now (until MPC restarts; Apply keeps it off)' : 'Turn this plugin on or off (applied with the rest)';
        t.onclick = live ? () => { S.rackMsg = ''; send({ t: 'rack_live', slot: s.id, index: f.index, name: f.name, on: !on }); }
                         : () => toggleEdit('enable', s.id, f, !on);
        const x = el('button', '', gone ? 'undo' : 'remove');
        x.onclick = () => toggleEdit('remove', s.id, f);
        row.append(t, x);
      }
      box.append(row);
    }
    for (const e of S.edits.filter(e => e.op === 'add' && e.slot === s.id)) {
      const row = el('div', 'rack-fx new');
      row.append(el('span', 'nm', e.plugin), el('span', 'tag', 'new'));
      const x = el('button', '', 'undo');
      x.onclick = () => { S.edits.splice(S.edits.indexOf(e), 1); S.rackConfirm = 0; renderRack(); };
      row.append(x);
      box.append(row);
    }
    const free = r.plugins.filter(p => !used.has(p.name));
    if (slotCount(s) < r.max_inserts && free.length) {
      const add = el('div', 'rack-add');
      const sel = el('select');
      for (const p of free) { const o = el('option', '', p.name); o.value = p.name; sel.append(o); }
      const b = el('button', '', 'add');
      b.onclick = () => { S.edits.push({ op: 'add', slot: s.id, plugin: sel.value }); S.rackConfirm = 0; S.rackMsg = ''; renderRack(); };
      add.append(sel, b);
      box.append(add);
    }
    body.append(box);
  }
  const bar = el('div', 'rack-bar');
  const offs = liveOffs(), count = S.edits.length + offs.length;
  const go = el('button', S.rackConfirm ? 'go' : '', S.rackConfirm ? 'Restart MPC and apply' : `Apply ${count} change${count === 1 ? '' : 's'}`);
  go.disabled = !count;
  go.onclick = () => {
    if (!S.rackConfirm) {
      S.rackConfirm = Date.now();
      setTimeout(() => { if (S.rackConfirm && Date.now() - S.rackConfirm >= 5900) { S.rackConfirm = 0; renderRack(); } }, 6000);
      renderRack();
      return;
    }
    S.rackConfirm = 0;
    S.rackMsg = 'Sending…';
    const edits = S.edits.map(e => e.op === 'add' ? { op: 'add', slot: e.slot, plugin: e.plugin }
      : e.op === 'remove' ? { op: 'remove', slot: e.slot, index: e.index, name: e.name }
      : { op: 'enable', slot: e.slot, index: e.index, name: e.name, value: e.value }).concat(offs);
    send({ t: 'rack_apply', edits });
    renderRack();
  };
  bar.append(go);
  if (S.edits.length) {
    const d = el('button', '', 'Discard');
    d.style.marginTop = '4px';
    d.onclick = () => { S.edits = []; S.rackConfirm = 0; S.rackMsg = ''; renderRack(); };
    bar.append(d);
  }
  if (S.rackMsg) bar.append(el('p', 'rack-msg', S.rackMsg));
  body.append(bar);
}
function rackResult(m) {
  if (m.ok) { S.rackApplying = true; S.edits = []; S.rackMsg = ''; }
  else S.rackMsg = `Not applied: ${m.error}`;
  renderRack();
}

// ---- layout ----
window.addEventListener('hashchange', () => { const id = fromHash(); if (id !== null && id !== S.sel) select(id); else render(); });
$('menu').onclick = () => document.body.classList.toggle('menu-open');
new ResizeObserver(() => redraw()).observe($('panel'));
window.matchMedia(`(resolution: ${window.devicePixelRatio}dppx)`).addEventListener?.('change', redraw);
// Labels need the skins' fonts; draw again once each arrives.
for (const f of ['400 20px "Titillium Web"', '600 20px "Titillium Web"', '700 20px "Titillium Web"', '700 20px "Roboto"'])
  document.fonts.load(f).then(redraw, () => {});

connect();
render();
