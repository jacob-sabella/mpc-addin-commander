# Commander protocol, version 1

The addin serves HTTP on its port (6730 by default, `port=` in `mpc_commander_addin.conf`). One WebSocket at `/ws`
carries JSON text frames, one message per frame. The addin pushes state; the app sends changes. Every message is an
object with a `t` field naming its type. Unknown types and unknown fields are ignored by both sides.

Parameter values are VST2 values: floats from 0 to 1. `text` is what the plugin shows for the value
(`effGetParamDisplay`), `name` and `label` come from `effGetParamName` and `effGetParamLabel`.

## Server to client

| `t` | Fields | When |
|---|---|---|
| `hello` | `protocol` (1), `addin` (version string), `device` `{model, mpc}` (`/proc/device-tree/model`, `/tmp/com.akaipro.mpc.version`), `poll_ms`, `text_ms`, `midi` (`{client}`: the sequencer client number of the addin's port, or `{error}`) | first message after the upgrade |
| `plugins` | `plugins`: array of plugin objects | right after `hello`, and on request (`list`) |
| `transport` | `playing`, `recording`, `tempo` (bpm from MPC's clock, which MPC sends stopped or playing; `null` until two beats were clocked), `bar`, `beat` (1-based, 4 beats a bar), `tick` (0..959 of the beat), `source` (`"clock"`, `"mmc"` or `""` before anything arrived) | right after `plugins`; then on every start, stop, continue, song position, MMC command or locate (a time code position, turned into bars at the clock's tempo), beat and tempo change |
| `midi_in` | `bytes` (the message; a sysex keeps its first 16 bytes), `ms` (the addin's clock) | anything but clock, start/stop/continue, song position and MMC arrives on the addin's port |
| `project` | see below | reply to `project` |
| `plugin_added` | `plugin`: a plugin object | MPC created an instance |
| `plugin_removed` | `id` | MPC closed it |
| `values` | `id`, `v`: array of `[index, value, text]`; `text` is `null` when it was not read | a value or text changed |
| `pong` | | reply to `ping` |
| `error` | `msg` | a bad client message |

A plugin object:

```json
{"id": 3, "name": "Chordsmith", "vendor": "jacob-sabella", "product": "Chordsmith", "uid": "4368536d",
 "so": "/storage/Synths/jacob-sabella - VST - Chordsmith/chordsmith.so", "skin": true, "synth": true,
 "params": [{"i": 0, "name": "Key", "label": "", "value": 0.0, "text": "C"}, ...]}
```

`id` is unique for the life of the MPC process (never reused). `skin` says whether the plugin has a `Plugin Skins/TUI.json`: in the folder of `so`, else in
`<vendor> - VST - <product>/` beside that folder, where MPC finds it by name (a `.so` loaded from `Synths/NAM/` with its
skin in `Synths/jacob-sabella - VST - NAM/`). `params` is complete and in VST index order.

## Client to server

| `t` | Fields | Effect |
|---|---|---|
| `set` | `id`, `i`, `value` | `setParameter(i, value)` on that instance, from the addin's thread, then `audioMasterAutomate` to MPC with the value the plugin now returns (as when the plugin moves a value itself), so MPC's screen follows; the next poll reports it to the clients |
| `subscribe` | `ids`: array | these instances get their text polled at `poll_ms`; the others at `text_ms`. Default: none |
| `list` | | a fresh `plugins` message |
| `ping` | | `pong` |
| `transport` | `cmd`: `play`, `stop`, `continue` or `record` | sent on the addin's port as MMC (`F0 7F 7F 06 <cmd> F7`: stop 01, play 02, record strobe 06 then play) and as MIDI real-time (`FA`, `FC`, `FB`), whichever `transport=` in the settings allows (default both; `record` needs MMC) |
| `midi` | `bytes`: 1..256 integers | one MIDI message out of the addin's port into MPC: a channel message, a song position, a real-time byte or a whole sysex |
| `surface` | `bytes`: 1..256 integers | appended as raw bytes to the control-surface injector file (`surface=` in the settings, `/data/midi.in` by default; an error when the file isn't there). The codes are the device's own: capture them with the injector's log |
| `project` | | a `project` message: a snapshot of the most recent project file |

## The sequencer port and MPC's settings

The addin opens an ALSA sequencer client named `MPC Commander` with a port `Out` (into MPC) and a port `In` (from
MPC). MPC lists it like any MIDI device, without a restart. For the app's MIDI and transport commands to reach MPC,
enable the port as a MIDI input in MPC's preferences, with Receive MMC on for MMC and Start/Stop Sync receive on for
real-time commands. For the app to follow MPC, enable the port as a MIDI output with clock sync and Send MMC. With
`midi=0` in the settings no port is opened and every message above answers with an error.

## The project snapshot

```json
{"t": "project", "path": "/storage/MPC Documents/Projects/Jam 1.xpj", "name": "Jam 1", "mtime": 1759500000,
 "version": 28, "key": "C Major", "master_tempo": 128.0, "master_tempo_enabled": false, "tempo": 93.5,
 "current_track": 1,
 "sequence": {"index": 1, "name": "Verse", "tempo": 93.5, "tempo_enabled": true, "bars": 8, "loop": true,
              "loop_start": 4, "loop_end": 8, "beats_per_bar": 4, "beat_length": 960},
 "tracks": [{"index": 1, "name": "Keys", "kind": 3, "type": "plugin", "colour": 1179392, "track_mute": false,
             "record_arm": false, "mute": false, "solo": false, "volume": 0.45, "pan": 0.5,
             "plugin": {"name": "Chordsmith", "vendor": "jacob-sabella", "format": "VST",
                        "file": "/storage/Synths/x/x.so", "preset": "Lydian Pad"}}],
 "stock": [{"where": "/data/tracks[3]/program/mixable/inserts/effects[0]/plugin/plugin", "name": "AIR Reverb",
            "vendor": "AIR Music Technology", "preset": "Plate", "state": "412.ABCD..."}]}
```

The file is the most recent project in MPC's settings (`recentProject1` in `settings=`), or `project=` when set. It
is what MPC last saved or loaded, not the live state: the app shows it as the project and refreshes it on request.
`tempo` is the master tempo when that is enabled, else the current sequence's. `kind` is the program type number
in the file; `type` names the ones seen so far (`drum` 0, `plugin` 3, `audio` 6, `return` 7, `submix` 8, `output` 9,
`input` 10, else `other`). `mute`, `solo`, `volume` and `pan` are the mixer's; `track_mute` is the sequencer track's.
`plugin` is `null` for a track without one. A missing or unreadable file is an `error` message (HTTP: 404).

`stock` lists every one of Akai's own plugins in the file (any object whose `description.pluginFormatName` is `MPC`,
wherever it sits: a track's instrument, an insert, a drum pad's insert, a return), in file order. `where` is its
path in the file's JSON, `state` the saved state as the file has it: JUCE's base64 (`<bytes>.<chars>`, alphabet
`.A-Za-z0-9+`, six bits per character, least significant first). The addin passes it through; the app decodes it.
The newer layout (AIR plugins) is `ACVS`, a size, the 64-byte engine name, then a count N and N float32 values in
0..1 in parameter order, then the preset. The older layout is a sorted list of name and float32 pairs. A state that
isn't JUCE base64 is left out. The values are those of the last save: the app's Sync button presses Save on the
control surface (`surface` with note 0x2A), the user picks Project on the MPC, and the app asks for `project` again.

## HTTP

| Route | |
|---|---|
| `GET /ws` with `Upgrade: websocket` | the socket above |
| `GET /info` | JSON: `version`, `protocol`, `plugins` (count), `clients` |
| `GET /plugins` | the `plugins` message as a document, for scripts |
| `GET /project` | the `project` message as a document |
| `GET /skin/<id>/<path>` | a file from that plugin's `Plugin Skins/` folder (found as for `skin` above), read-only; `..` and symlinks out of the folder are refused; `ETag` is the file's size and mtime, `If-None-Match` gives 304 |
| `GET /stock/<folder>/<path>` | a file from a stock plugin's `Plugin Skins/` folder, read-only. `<folder>` is `<vendor> - MPC - <name>` (URL-escaped), looked up in `/usr/share/Akai/Content/Synths`, `/storage/Synths`, `/sdcard/Synths`, `/media/az01-internal/Synths` and `/media/*/Synths`, the first with a `TUI.json`; a file the skin names but doesn't hold comes from `AIR Components` or `AKAI Components` beside it or in `/usr/share/Akai/Content/Synths`, as MPC finds it; the same confinement and `ETag` as `/skin`. Akai's files stay on the device: the app caches them locally and never ships them |
| `GET /` | a short page naming the addin and its version |

Every HTTP reply but the WebSocket is `Connection: close`.

## Rules the addin keeps

- Nothing here runs on MPC's audio threads except two flag stores (an instance's `audioMasterAutomate` and
  `audioMasterUpdateDisplay` calls are noted and the poll thread reads that instance next).
- `effGetChunk` is never called by the addin: its buffer is shared with MPC's Save.
- At most `max_clients` connections at once (default 6), each on its own thread.
- The sequencer port is an ordinary MIDI device to MPC: nothing is injected into MPC's internals. Transport state is
  what MPC chose to send on that port.
