// A plugin skin (TUI.json, see docs/SKIN.md) resolved into pages, and a page laid out as a flat draw list and a flat
// control list from live parameter values. The browser's copy of app/crates/commander-skin: same rules, same output, so
// the browser and the desktop app draw a page alike. No DOM here; app.js draws the items.

export const PAGE_WIDTH = 1280;
export const PAGE_HEIGHT = 628;

export const rect = (x, y, w, h) => ({ x, y, w, h });
export const contains = (r, x, y) => x >= r.x && y >= r.y && x < r.x + r.w && y < r.y + r.h;

// "x y w h"; missing or malformed numbers read as 0.
export function parseRect(s) {
  const n = String(s || '').trim().split(/\s+/).map(v => (/^[-+]?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?$/.test(v) ? +v : 0));
  return rect(n[0] || 0, n[1] || 0, n[2] || 0, n[3] || 0);
}

// aarrggbb or rrggbb hex; anything else is opaque light grey.
export function parseColour(s) {
  s = String(s || '').trim();
  const ok = /^[0-9a-fA-F]+$/.test(s);
  const h = i => parseInt(s.slice(i, i + 2), 16);
  if (ok && s.length === 8) return { a: h(0), r: h(2), g: h(4), b: h(6) };
  if (ok && s.length === 6) return { a: 255, r: h(0), g: h(2), b: h(4) };
  return { a: 255, r: 200, g: 200, b: 200 };
}

export const css = c => `rgba(${c.r},${c.g},${c.b},${(c.a / 255).toFixed(3)})`;

// JUCE justification words, reduced to two axes: h left|centre|right, v top|centre|bottom.
export function parseJustification(s) {
  const j = { h: 'centre', v: 'centre' };
  for (const w of String(s || '').split(/\s+/)) {
    if (w === 'left' || w === 'right') j.h = w;
    else if (w === 'top' || w === 'bottom') j.v = w;
  }
  return j;
}

// The option an N-option parameter is on: round(value * (N - 1)).
export function optionIndex(value, count) {
  const last = Math.max(count - 1, 0);
  const i = Math.round(value * last);
  if (!Number.isFinite(i) || i < 0) return 0;
  return Math.min(i, last);
}

const parseParameter = s => {
  const m = /^\s*Parameter\s+(\d+)\s*$/.exec(String(s || ''));
  return m ? +m[1] : null;
};

// IndexedEnabling/<index>/<count>/Parameter <param>: shown while the parameter is on option index of count.
function parseCondition(h) {
  const m = /^IndexedEnabling\/(\d+)\/(\d+)\/(.*)$/.exec(String(h || ''));
  if (!m) return null;
  const param = parseParameter(m[3]);
  return param === null ? null : { index: +m[1], count: +m[2], param };
}
const holds = (c, params) => optionIndex(params.value(c.param), c.count) === c.index;

// rel relative to the folder of base, both relative to the skin folder: ("../../A/x.json", "../B/y.json") gives
// "../../B/y.json". null for an absolute path.
export function resolvePath(base, rel) {
  if (rel.startsWith('/') || rel.includes('\\')) return null;
  const parts = base.split('/');
  parts.pop();
  for (const c of rel.split('/')) {
    if (c === '' || c === '.') continue;
    if (c === '..') {
      if (parts.length && parts[parts.length - 1] !== '..') parts.pop();
      else parts.push('..');
    } else parts.push(c);
  }
  return parts.join('/');
}

// x1, y1, x2, y2: expressions of numbers, + - * /, brackets and <sibling>.<edge> over siblings placed earlier.
function relativeRect(expr, placed) {
  const v = [];
  for (const e of expr.split(',')) {
    const x = evalRel(e, placed);
    if (x === null) return null;
    v.push(x);
  }
  return v.length === 4 ? rect(v[0], v[1], v[2] - v[0], v[3] - v[1]) : null;
}

function evalRel(s, placed) {
  let i = 0;
  const skip = () => { while (i < s.length && /\s/.test(s[i])) i++; };
  const eat = c => { skip(); if (s[i] === c) { i++; return true; } return false; };
  const sum = () => {
    let v = product();
    while (v !== null) {
      if (eat('+')) { const r = product(); v = r === null ? null : v + r; }
      else if (eat('-')) { const r = product(); v = r === null ? null : v - r; }
      else break;
    }
    return v;
  };
  const product = () => {
    let v = atom();
    while (v !== null) {
      if (eat('*')) { const r = atom(); v = r === null ? null : v * r; }
      else if (eat('/')) { const r = atom(); v = r === null ? null : v / r; }
      else break;
    }
    return v;
  };
  const atom = () => {
    if (eat('(')) { const v = sum(); return v !== null && eat(')') ? v : null; }
    if (eat('-')) { const v = atom(); return v === null ? null : -v; }
    skip();
    const start = i;
    if (i >= s.length) return null;
    if (/[0-9.]/.test(s[i])) {
      while (i < s.length && /[0-9.]/.test(s[i])) i++;
      const t = s.slice(start, i);
      return /^(\d+\.?\d*|\.\d+)$/.test(t) ? parseFloat(t) : null;
    }
    while (i < s.length && /[A-Za-z0-9_.]/.test(s[i])) i++;
    const word = s.slice(start, i);
    const dot = word.lastIndexOf('.');
    if (dot < 0) return null;
    const r = placed.get(word.slice(0, dot));
    if (!r) return null;
    switch (word.slice(dot + 1)) {
      case 'left': case 'x': return r.x;
      case 'top': case 'y': return r.y;
      case 'right': return r.x + r.w;
      case 'bottom': return r.y + r.h;
      case 'width': return r.w;
      case 'height': return r.h;
      default: return null;
    }
  };
  const v = sum();
  skip();
  return v !== null && i === s.length ? v : null;
}

function labelStyle(data) {
  const ts = (data && data.textStyle) || {};
  const font = ts.font || {};
  const str = (o, k) => (typeof o[k] === 'string' ? o[k] : '');
  return {
    font: { name: str(font, 'name'), style: str(font, 'style'), height: typeof font.height === 'number' ? font.height : 20 },
    colour: parseColour(str(ts, 'colour')),
    justification: parseJustification(str(ts, 'justification')),
    uppercase: str(ts, 'case') === 'Upper Case',
  };
}

function convertComponent(c) {
  const cd = c.componentData || {};
  const data = cd.data && typeof cd.data === 'object' ? cd.data : {};
  const str = k => (typeof data[k] === 'string' ? data[k] : '');
  const num = (k, d) => (typeof data[k] === 'number' ? data[k] : d);
  const handle = () => str('handleName') || 'Data';
  const kind = cd.type || '';
  let w;
  switch (kind) {
    case 'Image': w = { t: 'image', file: str('image') }; break;
    case 'Knob':
      if (str('knobType') === 'ValueSlider') w = { t: 'valuetext', style: labelStyle(data), handle: handle() };
      else w = { t: 'knob', file: str('filmStrip'), frames: Math.max(1, Math.trunc(num('numFrames', 1))),
                 invert: data.invert === true, handle: handle() };
      break;
    case 'Button':
      w = { t: 'button', on: str('onImage'), off: str('offImage'), id: Math.max(0, Math.trunc(num('buttonId', 0))),
            group: Math.max(1, Math.trunc(num('numButtonsInGroup', 1))), handle: handle(), lamp: false };
      break;
    case 'Indicator':
      w = { t: 'button', on: str('onImage'), off: str('offImage'), id: Math.max(0, Math.trunc(num('indicatorId', 0))),
            group: Math.max(1, Math.trunc(num('numIndicatorsInGroup', 1))), handle: handle(), lamp: true };
      break;
    // Only the down arrow has been seen; other shapes draw nothing rather than a placeholder.
    case 'Decorator':
      w = str('type') === 'Down Arrow' ? { t: 'arrow', colour: parseColour(str('foregroundColour')) } : { t: 'focus' };
      break;
    case 'Label':
      w = { t: 'label', style: labelStyle(data), kind: str('type') === 'Name' ? 'name' : 'value', handle: handle() };
      break;
    case 'Slider':
      w = { t: 'slider', thumb: str('thumbImage'), vertical: str('direction') !== 'Horizontal', handle: handle() };
      break;
    case 'Focus': w = { t: 'focus' }; break;
    case 'Meter': w = { t: 'meter', handle: handle() }; break;
    // Anything else names a definition; one that does not exist draws as generic.
    default:
      w = { t: 'instance', def: kind,
            map: ((c['handle remapping'] || {}).map || []).map(m => [String(m.key || ''), String(m.value || '')]) };
  }
  const b = c.bounds || {};
  return {
    name: String(cd.name || ''),
    rect: parseRect(b.bounds),
    proportional: b.boundsType === 'Proportional',
    relative: b.boundsType === 'Relative' ? String(b.relativeBounds || '') : null,
    always: b.whenVisible !== 'WhenFocussed',
    conditions: (b.additionalInvalidatingHandles || []).map(parseCondition).filter(Boolean),
    widget: w,
  };
}

function convertDefinition(d) {
  d = d || {};
  const actions = d.actions || [];
  const toggles = actions.some(a => a.handler === 'Toggle Switch');
  const qlink = actions.some(a => a.handler === 'Q-Link');
  // The background becomes two children drawn first, filling the definition's bounds.
  const children = [];
  const bg = d.backgroundData && d.backgroundData.unfocussed;
  if (bg) {
    const whole = widget => ({ name: '', rect: rect(0, 0, 1, 1), proportional: true, relative: null, always: true,
                               conditions: [], widget });
    // A JUCE colour as hex, 0 being transparent: the shared libraries' components use it.
    const argb = /^[0-9a-fA-F]{1,8}$/.test(String(bg.colour || '').trim()) ? parseInt(bg.colour.trim(), 16) : 0;
    if (argb >>> 24 > 0)
      children.push(whole({ t: 'fill', colour: { a: argb >>> 24, r: (argb >>> 16) & 255, g: (argb >>> 8) & 255, b: argb & 255 } }));
    if (bg.image) children.push(whole({ t: 'image', file: String(bg.image) }));
  }
  for (const c of d.componentsData || []) children.push(convertComponent(c));
  return { toggles, qlink, children };
}

// Makes a widget's images relative to the skin folder, from the folder of file.
function rebase(w, file) {
  const fix = k => { if (w[k]) { const p = resolvePath(file, w[k]); if (p !== null) w[k] = p; } };
  if (w.t === 'image' || w.t === 'knob') fix('file');
  else if (w.t === 'slider') fix('thumb');
  else if (w.t === 'button') { fix('on'); fix('off'); }
}

// A placed instance's handle map: Parameter N binds directly, any other value names a handle of the enclosing instance.
function resolveMap(entries, outer) {
  const map = new Map();
  for (const [key, value] of entries) {
    let p = parseParameter(value);
    if (p === null && outer.has(value)) p = outer.get(value);
    if (p !== null && p !== undefined) map.set(key, p);
  }
  return map;
}

// What a press on a placed instance does, and whether its button children set values: a Toggle Switch action flips the
// Data parameter; a Q-Link action selects it (a knob or slider child makes it draggable, button children set their own
// values); no action swallows the press.
function instanceGesture(def, map, params) {
  if (def.toggles) return [{ kind: 'toggle' }, false];
  if (!def.qlink) return [{ kind: 'inert' }, false];
  const visible = n => n.always && n.conditions.every(c => holds(c, params));
  const knob = def.children.find(n => visible(n) && ['knob', 'slider', 'valuetext'].includes(n.widget.t) &&
                                     map.has(n.widget.handle));
  if (knob) return [{ kind: 'drag', height: knob.rect.h }, false];
  const button = def.children.some(n => visible(n) && n.widget.t === 'button' && !n.widget.lamp && map.has(n.widget.handle));
  return [{ kind: 'select' }, button];
}

export class Skin {
  // tui: the parsed TUI.json. Throws when a tab names a missing definition.
  constructor(tui) {
    const data = (tui && tui.pageData) || {};
    const defs = data.componentDefinitions || {};
    this.definitions = new Map();
    for (const kv of defs.localComponentDefinitions || []) this.definitions.set(String(kv.key), convertDefinition(kv.value));
    this.pages = [];
    (data.tabs || []).forEach((tab, i) => {
      let name = String(tab.componentName || '');
      // An inline definition gets a key no file can use.
      if (tab.componentDefinition) {
        name = `\0tab ${i}`;
        this.definitions.set(name, convertDefinition(tab.componentDefinition));
      }
      if (!this.definitions.has(name)) throw new Error(`page "${tab.tabName || ''}" uses the missing definition "${name}"`);
      let size = parseRect(tab.initialSize);
      if (size.w <= 0 || size.h <= 0) size = rect(0, 0, PAGE_WIDTH, PAGE_HEIGHT);
      this.pages.push({ name: String(tab.tabName || ''), fnKey: tab.fnKeyIndex | 0, subIndex: tab.fnKeySubIndex | 0, size,
                        definition: name });
    });
    this.pages.sort((a, b) => a.fnKey - b.fnKey || a.subIndex - b.subIndex);
    this.imports = (defs.importFiles || []).map(f => resolvePath('TUI.json', String(f))).filter(p => p !== null);
  }

  // Adds an imported file's definitions the skin doesn't define itself, its images made relative to the skin folder.
  // Returns the files it imports in turn.
  addLibrary(path, lib) {
    const defs = (lib && lib.componentDefinitions) || {};
    for (const kv of defs.localComponentDefinitions || []) {
      if (this.definitions.has(kv.key)) continue;
      const def = convertDefinition(kv.value);
      for (const n of def.children) rebase(n.widget, path);
      this.definitions.set(String(kv.key), def);
    }
    return (defs.importFiles || []).map(f => resolvePath(path, String(f))).filter(p => p !== null);
  }

  reachable() {
    const out = [], seen = new Set(), todo = this.pages.map(p => p.definition);
    while (todo.length) {
      const name = todo.pop();
      if (seen.has(name)) continue;
      seen.add(name);
      const def = this.definitions.get(name);
      if (!def) continue;
      for (const n of def.children) if (n.widget.t === 'instance') todo.push(n.widget.def);
      out.push(def);
    }
    return out;
  }

  // Every image the pages can draw, relative to the skin folder; definitions no page reaches are left out.
  imageFiles() {
    const out = new Set();
    for (const def of this.reachable())
      for (const { widget: w } of def.children) {
        if (w.t === 'image' || w.t === 'knob') out.add(w.file);
        else if (w.t === 'slider') out.add(w.thumb);
        else if (w.t === 'button') { out.add(w.on); out.add(w.off); }
      }
    out.delete('');
    return [...out].sort();
  }

  // The draw list and control list of page `page`. params: {value(i), name(i), text(i)}.
  layout(page, params) {
    const out = { items: [], controls: [] };
    const p = this.pages[page];
    if (p) this.walk(p.definition, rect(0, 0, PAGE_WIDTH, PAGE_HEIGHT), new Map(), false, params, 0, out);
    return out;
  }

  walk(defName, parent, map, buttonsSet, params, depth, out) {
    if (depth > 16) return;   // definitions only nest so deep before the file is a cycle
    const def = this.definitions.get(defName);
    if (!def) return;
    // Each child's bounds in the parent's own coordinates, for Relative siblings after it.
    const placed = new Map([['parent', rect(0, 0, parent.w, parent.h)]]);
    for (const node of def.children) {
      let local;
      if (node.relative !== null) local = relativeRect(node.relative, placed) || rect(0, 0, 0, 0);
      else if (node.proportional) {
        const r = node.rect;
        local = rect(r.x * parent.w, r.y * parent.h, r.w * parent.w, r.h * parent.h);
      } else local = node.rect;
      if (!placed.has(node.name)) placed.set(node.name, local);
      if (!node.always || !node.conditions.every(c => holds(c, params))) continue;
      const r = rect(local.x + parent.x, local.y + parent.y, local.w, local.h);
      const w = node.widget;
      const bound = h => (map.has(h) ? map.get(h) : null);
      const value = p => (p === null ? 0 : params.value(p));
      switch (w.t) {
        case 'image': out.items.push({ t: 'image', rect: r, file: w.file }); break;
        case 'knob': {
          const param = bound(w.handle);
          let frame = optionIndex(value(param), w.frames);
          if (w.invert) frame = Math.max(0, w.frames - 1 - frame);
          out.items.push({ t: 'knob', rect: r, file: w.file, frames: w.frames, frame, param });
          break;
        }
        case 'arrow': out.items.push({ t: 'arrow', rect: r, colour: w.colour }); break;
        case 'fill': out.items.push({ t: 'fill', rect: r, colour: w.colour }); break;
        case 'button': {
          const param = bound(w.handle);
          const v = value(param);
          const lit = w.group > 1 ? optionIndex(v, w.group) === w.id : v >= 0.5;
          out.items.push({ t: 'button', rect: r, file: lit ? w.on : w.off, on: lit, param });
          if (buttonsSet && param !== null && !w.lamp)
            out.controls.push({ rect: r, param, gesture: { kind: 'set', value: w.group > 1 ? w.id / (w.group - 1) : 1 },
                                name: node.name });
          break;
        }
        case 'slider': {
          const param = bound(w.handle);
          out.items.push({ t: 'slider', rect: r, thumb: w.thumb, vertical: w.vertical, value: value(param), param });
          break;
        }
        case 'label': {
          const param = bound(w.handle);
          let text = param === null ? '' : w.kind === 'name' ? params.name(param) : params.text(param);
          if (w.style.uppercase) text = text.toUpperCase();
          out.items.push({ t: 'label', rect: r, text, style: w.style, param });
          break;
        }
        case 'valuetext': {
          const param = bound(w.handle);
          out.items.push({ t: 'label', rect: r, text: param === null ? '' : params.text(param), style: w.style, param });
          break;
        }
        case 'focus': break;
        case 'meter': {
          const param = bound(w.handle);
          out.items.push({ t: 'meter', rect: r, value: value(param), param });
          break;
        }
        case 'instance': {
          const inner = resolveMap(w.map, map);
          const data = inner.has('Data') ? inner.get('Data') : null;
          const cd = this.definitions.get(w.def);
          if (!cd) {
            out.items.push({ t: 'generic', rect: r, kind: w.def, name: node.name, param: data });
            break;
          }
          const [gesture, setButtons] = instanceGesture(cd, inner, params);
          out.controls.push({ rect: r, param: data, gesture, name: node.name });
          this.walk(w.def, r, inner, setButtons, params, depth + 1, out);
          break;
        }
      }
    }
  }
}

// The topmost control at a point (later controls are on top).
export function hit(controls, x, y) {
  for (let k = controls.length - 1; k >= 0; k--) if (contains(controls[k].rect, x, y)) return controls[k];
  return null;
}

// What a press on a control asks for: {set: value} once, {drag: {height, start}} or null.
export function press(control, params) {
  const p = control.param;
  if (p === null || p === undefined) return null;
  const g = control.gesture;
  if (g.kind === 'drag') return { param: p, drag: { height: g.height, start: params.value(p) } };
  if (g.kind === 'set') return { param: p, set: g.value };
  if (g.kind === 'toggle') return { param: p, set: params.value(p) < 0.5 ? 1 : 0 };
  return null;
}

// The font a label asks for, as the app resolves it: Titillium Web (Regular, SemiBold, Bold) or Roboto (Bold only),
// sized as JUCE sizes a font, by ascent plus descent: 1.521 em for Titillium Web, 1.172 em for Roboto.
export function fontFor(spec) {
  const name = spec.name.toLowerCase(), style = spec.style.toLowerCase();
  if (name.includes('roboto')) return { family: 'Roboto', weight: 700, em: spec.height / 1.172 };
  const weight = !name.includes('titillium') ? 400 : style.includes('semi') ? 600 : style.includes('bold') ? 700 : 400;
  return { family: 'Titillium Web', weight, em: spec.height / 1.521 };
}
