// The browser UI's skin renderer (web/skin.js) against the same Chordsmith fixture and the same expectations as the
// desktop app's crate (app/crates/commander-skin/tests/chordsmith.rs), so the two draw a page alike. Node 18+, no
// packages: node tests/test_web.mjs
import { readFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';
import assert from 'node:assert/strict';
import { Skin, hit, press, parseColour, resolvePath, fontFor, PAGE_WIDTH, PAGE_HEIGHT } from '../web/skin.js';

const here = dirname(fileURLToPath(import.meta.url));
const FIX = join(here, '..', 'app', 'crates', 'commander-skin', 'tests', 'fixtures', 'chordsmith');
const skin = () => new Skin(JSON.parse(readFileSync(join(FIX, 'TUI.json'), 'utf8')));

// Every parameter at value, named P<n> with text T<n>; overrides {index: value}.
const params = (value, over = {}) => ({
  value: i => (i in over ? over[i] : i < 128 ? value : 0),
  name: i => (i < 128 ? `P${i}` : ''),
  text: i => (i < 128 ? `T${i}` : ''),
});

let failed = 0;
const test = (name, f) => {
  try { f(); console.log(`ok   ${name}`); } catch (e) { failed++; console.log(`FAIL ${name}\n${e.stack}`); }
};

test('pages are the F-keys', () => {
  const s = skin();
  assert.deepEqual(s.pages.map(p => p.name), ['PLAY', 'VOICE', 'BUTTONS', 'PERFORM', 'SETUP']);
  s.pages.forEach((p, i) => {
    assert.equal(p.fnKey, i);
    assert.equal(p.subIndex, 0);
    assert.equal(p.size.w, PAGE_WIDTH);
    assert.equal(p.size.h, PAGE_HEIGHT);
  });
});

test('every page draws without generic items', () => {
  const s = skin();
  s.pages.forEach((page, i) => {
    const items = s.layout(i, params(0)).items;
    assert.ok(items.length > 10, `${page.name}: ${items.length}`);
    assert.equal(items.filter(it => it.t === 'generic').length, 0);
    const first = items.find(it => it.t !== 'fill');
    assert.equal(first.t, 'image');
    assert.deepEqual(first.rect, { x: 0, y: 0, w: PAGE_WIDTH, h: PAGE_HEIGHT });
    assert.ok(first.file.startsWith('sh_bg_'), first.file);
    for (const it of items) {
      const r = it.rect;
      assert.ok(r.x >= 0 && r.y >= 0 && r.x + r.w <= PAGE_WIDTH + 0.5 && r.y + r.h <= PAGE_HEIGHT + 0.5, JSON.stringify(it));
    }
  });
});

test('every named image exists', () => {
  const s = skin();
  const files = s.imageFiles();
  assert.ok(files.length > 100, String(files.length));
  for (const f of files) assert.ok(existsSync(join(FIX, f)), `${f} missing`);
  for (let i = 0; i < s.pages.length; i++)
    for (const it of s.layout(i, params(1)).items)
      if (['image', 'knob', 'button'].includes(it.t)) assert.ok(files.includes(it.file), it.file);
});

test('IndexedEnabling shows the popup list', () => {
  const s = skin();
  const popupButtons = items => items.filter(it => it.t === 'button' && it.file.startsWith('sh_popopt_0_key_')).length;
  const panel = items => items.some(it => it.t === 'image' && it.file === 'sh_pop_0_key.png');
  const closed = s.layout(0, params(0)).items;
  assert.equal(popupButtons(closed), 0);
  assert.ok(!panel(closed));
  const open = s.layout(0, params(0, { 73: 1 })).items;
  assert.equal(popupButtons(open), 12);
  assert.ok(panel(open));
  const panelPos = open.findIndex(it => it.t === 'image' && it.file === 'sh_pop_0_key.png');
  const firstButton = open.findIndex(it => it.t === 'button' && it.file.startsWith('sh_popopt_0_key_'));
  assert.ok(panelPos < firstButton, 'the panel is drawn under its options');
  assert.ok(!open.some(it => it.t === 'image' && it.file === 'sh_pop_0_scale.png'));
});

test('knob frame follows the value', () => {
  const s = skin();
  const knob = v => s.layout(1, params(v)).items.filter(it => it.t === 'knob');
  const at0 = knob(0);
  assert.equal(at0.length, 2);
  for (const k of at0) {
    assert.equal(k.file, 'sh_knob_r54.png');
    assert.equal(k.frames, 127);
    assert.equal(k.frame, 0);
    assert.notEqual(k.param, null);
  }
  assert.ok(knob(0.5).every(k => k.frame === 63));
  assert.ok(knob(1).every(k => k.frame === 126));
  const p = params(0.25);
  const drags = s.layout(1, p).controls.filter(c => c.gesture.kind === 'drag');
  assert.equal(drags.length, 2);
  const a = press(drags[0], p);
  assert.equal(a.drag.height, 118);
  assert.equal(a.drag.start, 0.25);
});

test('inverted knob counts down', () => {
  const s = new Skin(JSON.parse(`{"pageData": {"tabs": [{"tabName": "A", "fnKeyIndex": 0, "fnKeySubIndex": 0,
    "componentName": "A|A", "initialSize": "0 0 1280 628"}],
    "componentDefinitions": {"importFiles": [], "localComponentDefinitions": [
      {"key": "A|A", "value": {"actions": [], "componentsData": [
        {"componentData": {"name": "k", "type": "knob", "data": {"handleName": "Data"}},
         "handle remapping": {"map": [{"key": "Data", "value": "Parameter 5"}]},
         "bounds": {"bounds": "10 20 100 100", "whenVisible": "Always", "additionalInvalidatingHandles": []}}]}},
      {"key": "knob", "value": {"actions": [{"onAction": "Mouse Down", "handler": "Q-Link", "handleName": "Data"}],
        "componentsData": [
        {"componentData": {"name": "Knob", "type": "Knob", "data": {"knobType": "FilmStrip", "filmStrip": "k.png",
           "numFrames": 11, "invert": true, "handleName": "Data"}},
         "bounds": {"bounds": "0 0 100 100", "whenVisible": "Always", "additionalInvalidatingHandles": []}}]}}
    ]}}}`));
  const frame = v => { const k = s.layout(0, params(v)).items[0]; return [k.frame, k.rect.x, k.rect.y]; };
  assert.deepEqual(frame(0), [10, 10, 20]);
  assert.deepEqual(frame(0.5), [5, 10, 20]);
  assert.deepEqual(frame(1), [0, 10, 20]);
});

test('button group lights the chosen option', () => {
  const s = skin();
  const lit = v => s.layout(0, params(0, { 14: v })).items.filter(it => it.t === 'button' && it.on && it.param === 14)
    .map(it => it.file);
  assert.deepEqual(lit(0), ['sh_seg_input_0_on.png']);
  assert.deepEqual(lit(0.5), ['sh_seg_input_1_on.png']);
  assert.deepEqual(lit(1), ['sh_seg_input_2_on.png']);
  assert.equal(s.layout(0, params(0)).items.filter(it => it.t === 'button' && !it.on && it.param === 14 &&
    it.file.startsWith('sh_seg_input_')).length, 2);
  const pill = v => s.layout(0, params(0, { 41: v })).items.find(it => it.t === 'button' && it.param === 41).file;
  assert.equal(pill(0), 'sh_pill_off.png');
  assert.equal(pill(0.49), 'sh_pill_off.png');
  assert.equal(pill(0.5), 'sh_pill_on.png');
});

test('labels carry text, font and colour', () => {
  const s = skin();
  const items = s.layout(0, params(0)).items;
  const name = items.find(it => it.t === 'label' && it.param === 41);
  assert.equal(name.text, 'P41');
  assert.equal(name.style.font.name, 'Titillium Web');
  assert.equal(name.style.font.style, 'SemiBold');
  assert.ok(Math.abs(name.style.font.height - 27.3) < 1e-3);
  assert.deepEqual(name.style.colour, { a: 255, r: 0xf3, g: 0xec, b: 0xff });
  assert.deepEqual(name.style.justification, { h: 'centre', v: 'centre' });
  assert.equal(name.style.uppercase, false);
  assert.deepEqual([name.rect.x, name.rect.y], [530, 82 + 34]);
  const value = items.find(it => it.t === 'label' && it.param === 0);
  assert.equal(value.text, 'T0');
  assert.deepEqual(value.style.colour, parseColour('ff2ad4c0'));
  assert.ok(s.layout(1, params(0)).items.some(it => it.t === 'label' && it.style.uppercase && it.text.startsWith('T')));
  assert.ok(items.some(it => it.t === 'label' && it.style.justification.h === 'left'));
  const f = fontFor(name.style.font);
  assert.equal(f.family, 'Titillium Web');
  assert.equal(f.weight, 600);
  assert.ok(Math.abs(f.em - 27.3 / 1.521) < 1e-6);
});

test('hits set, toggle and respect visibility', () => {
  const s = skin();
  const p = params(0);
  let controls = s.layout(0, p).controls;
  assert.deepEqual(press(hit(controls, 300, 120), p), { param: 14, set: 0.5 });
  const toggle = hit(controls, 640, 119);
  assert.equal(toggle.gesture.kind, 'toggle');
  assert.deepEqual(press(toggle, p), { param: 41, set: 1 });
  assert.deepEqual(press(toggle, params(0, { 41: 1 })), { param: 41, set: 0 });
  assert.deepEqual(press(hit(controls, 850, 120), p), { param: 73, set: 1 });
  const under = hit(controls, 856, 258);
  const a = under && press(under, p);
  assert.ok(!(a && a.param === 0 && 'set' in a));
  const open = params(0, { 73: 1 });
  controls = s.layout(0, open).controls;
  const o = press(hit(controls, 856, 258), open);
  assert.equal(o.param, 0);
  assert.ok(Math.abs(o.set - 2 / 11) < 1e-6);
  assert.equal(hit(controls, 5, 600), null);
});

test('relative bounds and imports', () => {
  assert.equal(resolvePath('TUI.json', '../../A/x.json'), '../../A/x.json');
  assert.equal(resolvePath('../../A/x.json', '../B/y.json'), '../../B/y.json');
  assert.equal(resolvePath('../../A/x.json', 'img/k.png'), '../../A/img/k.png');
  assert.equal(resolvePath('TUI.json', '/abs.json'), null);
  const s = new Skin({ pageData: { tabs: [{ tabName: 'A', componentName: 'P' }], componentDefinitions: {
    importFiles: ['../../Lib/lib.json'],
    localComponentDefinitions: [{ key: 'P', value: { componentsData: [
      { componentData: { name: 'Label', type: 'Image', data: { image: 'a.png' } }, bounds: { bounds: '100 50 300 40' } },
      { componentData: { name: 'Arrow', type: 'Decorator', data: { type: 'Down Arrow', foregroundColour: 'ffff0000' } },
        bounds: { boundsType: 'Relative', relativeBounds: 'Label.right - (Label.height * .5), Label.top, Label.right, Label.bottom' } },
      { componentData: { name: 'm', type: 'menu' }, 'handle remapping': { map: [{ key: 'Data', value: 'Parameter 2' }] },
        bounds: { bounds: '0 0 .5 .5', boundsType: 'Proportional' } },
    ] } }] } } });
  assert.deepEqual(s.imports, ['../../Lib/lib.json']);
  let items = s.layout(0, params(0)).items;
  assert.deepEqual(items[1], { t: 'arrow', rect: { x: 380, y: 50, w: 20, h: 40 }, colour: { a: 255, r: 255, g: 0, b: 0 } });
  assert.equal(items[2].t, 'generic');   // menu isn't defined yet
  assert.deepEqual(items[2].rect, { x: 0, y: 0, w: 640, h: 314 });
  const more = s.addLibrary('../../Lib/lib.json', { componentDefinitions: { importFiles: ['../Other/o.json'],
    localComponentDefinitions: [{ key: 'menu', value: { actions: [{ handler: 'Toggle Switch' }],
      backgroundData: { unfocussed: { colour: 'ff102030', image: 'bg.png' } }, componentsData: [] } }] } });
  assert.deepEqual(more, ['../../Other/o.json']);
  const out = s.layout(0, params(0));
  items = out.items;
  assert.equal(items[2].t, 'fill');
  assert.deepEqual(items[3], { t: 'image', rect: { x: 0, y: 0, w: 640, h: 314 }, file: '../../Lib/bg.png' });
  assert.equal(out.controls[0].gesture.kind, 'toggle');
  assert.equal(out.controls[0].param, 2);
});

if (failed) { console.log(`FAILED: ${failed}`); process.exit(1); }
console.log('PASSED');
