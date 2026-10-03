# Rendering a plugin's skin on the desktop

A port built with mpc-vst-plugins ships `Plugin Skins/TUI.json` and PNGs next to its `.so`. MPC draws the plugin's
pages from those files and the plugin's live parameter values and display text. The desktop app fetches the same
files from the addin (`GET /skin/<id>/<path>`, see PROTOCOL.md) and draws the same thing from the same data. This
file is the subset of the format the app renders, as `tools/shadow_skin.py write_skin()` in mpc-vst-plugins emits
it and as verified on devices (mpc-vst-plugins `docs/NOTES.md`, "Skin components"). Anything outside it draws the
generic widget for the bound parameter, never a blank.

## Files

- `TUI.json`: the pages. Its `pageData.tabs[]` are the pages (`tabName`, `fnKeyIndex` = the F-key the page is on,
  `fnKeySubIndex` = nested page under that F-key, `componentName` = the key of the page's component definition,
  `initialSize` = `"0 0 1280 628"`: the page area, in device pixels, below MPC's own top bar).
- `pageData.componentDefinitions.localComponentDefinitions`: a list of `{key, value}`. A value is a component
  definition: `actions[]`, `backgroundData` (`focussed`/`unfocussed`: `colour` and `image`, `"0"` = none), and
  `componentsData[]`, its children. The page's definition is the one whose key is the tab's `componentName`.
- `componentDefinitions.importFiles`: Akai's generic overlay definitions (knob and menu overlays, device UI the app
  does not reproduce). Ignore.
- `Q-Links.json`: which parameter each of the 16 Q-Links drives per page (`Tab`/`SubTab` 1-based,
  `"Q-Link n": <param index or -1>`). Optional for the app (a Q-Link strip under the panel).
- The PNGs the components name, relative to the skin folder.

## A child component

```json
{"componentData": {"name": "...", "type": "<type>", "data": {...}},
 "handle remapping": {"map": [{"key": "Data", "value": "Parameter 7"}, {"key": "Text", "value": "Parameter 9"}]},
 "bounds": {"bounds": "x y w h", "whenVisible": "Always" | "WhenFocussed",
            "additionalInvalidatingHandles": ["IndexedEnabling/<i>/<N>/Parameter <p>"]}}
```

- `bounds` is relative to the parent's origin. The page's children are in page coordinates.
- `type` is either a built-in (below) or the key of a local component definition: a **placed instance** of that
  definition, drawn at `bounds` with the definition's children inside it.
- Handles bind a component to parameters. `Parameter <n>` is the VST index. A placed instance's `handle remapping`
  sets its `Data` handle (and sometimes `Text`); children inside the definition name the handle they use
  (`data.handleName`, `Data` or `Text`), resolved against the enclosing instance's map. A child with no mapping
  for its handle has no parameter: draw it static.
- `whenVisible: "WhenFocussed"` (the `Focus` ring) is not drawn: the app has no device focus.
- `additionalInvalidatingHandles` with `IndexedEnabling/<i>/<N>/Parameter <p>`: draw the component only while
  `round(value(p) * (N - 1)) == i`. This is how popup lists and mode panels appear.

## Built-in types and how to draw them

| `type` | `data` | Drawing |
|---|---|---|
| `Image` | `image` | the PNG at `bounds`, stretched to `w h` (they always match the file) |
| `Button` | `onImage`, `offImage`, `buttonId` b, `numButtonsInGroup` N | `on` when N > 1 and `round(value * (N - 1)) == b`, or N == 1 and `value >= 0.5`; draw the matching image. The on and off image may be the same file (a stepper arrow) |
| `Knob` | `knobType: "FilmStrip"`, `filmStrip`, `numFrames` F, `invert` | the strip is F square frames stacked vertically (frame height = image width); frame = `round(value * (F - 1))`, F-1 minus that when `invert` |
| `Label` | `textStyle: {font: {name, style, height}, colour, justification, case}`, `type: "Name"` or `"Value"` | `Name`: the bound parameter's name. `Value`: its display text. `colour` is `aarrggbb` hex. `case: "Upper Case"` upper-cases. `justification` is JUCE's: `left`, `right`, `horizontallyCentred` and `top`, `bottom`, `verticallyCentred`. `height` is the font height in px. Fonts: `Titillium Web` and `Roboto` only (bundle both; style `SemiBold`, `Regular`, `Bold`, `Light`) |
| `Focus` | | nothing |
| `Meter` | | draw the generic meter bar for the bound parameter |

Draw order is the list order; later children cover earlier ones. The page's first child is normally the
background `Image`.

## Input: what a touch does on the device, and what the app sends

A definition's `actions[]` say what a touch of the instance does. The app does the same through `set`:

| `onAction` | `handler` | Meaning | App |
|---|---|---|---|
| `Mouse Down` | `Q-Link` | select this control (and, for a `Button` in a group of N, set the value to `buttonId / (N - 1)`; N == 1: 1) | click on a button child: `set` that value. On a `Knob`: drag vertically, `set` continuously (full height = 1.0). Elsewhere: select only |
| `Mouse Down` / `Enter Pressed` | `Toggle Switch` | flip the `Data` parameter between 0 and 1 | click: `set` 1 if value < 0.5 else 0 |
| `Double Click` / `Enter Pressed` | `Show Overlay` | the device's knob overlay | nothing |

Momentary parameters (triggers, stepper arrows) are `Toggle Switch` controls whose plugin drops them back to 0
by itself; the `values` push shows the drop. A popup field is a `Toggle Switch` on a hidden `<key>__open`
parameter; its list is a radio group of `Button`s on the real parameter, visible through `IndexedEnabling/1/2`.
Picking an option sets the parameter; the plugin closes the popup itself.

## Generic panel

When a plugin ships no skin, or for a component the renderer does not know, draw from the parameter list alone:
a grid of knobs with name, value arc and text, parameters whose text is one of a small set of strings as a
segmented control, text-only readouts as labels.
