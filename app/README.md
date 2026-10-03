# Commander desktop app

The desktop half of the Commander addin: it connects to the addin's WebSocket on the device,
lists the plugin instances MPC has open, draws the selected one's screen skin from the same
`TUI.json` and PNGs the device uses, and sends parameter changes back. Rust, egui on wgpu.

## Build

    cd app
    cargo build --release          # target/release/commander
    cargo test                     # protocol, skin and UI tests, no device or display needed
    cargo run -- --once            # renders one headless frame and exits 0 (what CI runs)

The pinned toolchain is in `rust-toolchain.toml` (stable, rustfmt, clippy); `rust-version` in
`Cargo.toml` is the floor the dependency tree needs. Linux draws through Vulkan; macOS through
Metal.

## Run

    cargo run --release -- --host mpc.local [--port 6730]

Type the device's address into the Host field and press Connect; `--host` sets it from the
command line. The last host and the window size are saved to
`~/.config/mpc-commander/config.toml` (`$XDG_CONFIG_HOME` is honoured); `connect_on_start`
there reconnects at launch. Skin files are cached under `~/.cache/mpc-commander/<plugin uid>/`
and revalidated with the addin's ETags, so a device that is offline still shows the panels it
showed last time. `RUST_LOG=debug` makes the process log chattier; the log pane at the bottom
keeps the last 200 lines.

## What the window shows

- Connection bar: host, port, Connect/Disconnect, the connection state, the ping round trip,
  and the addin's version and device model from `hello`.
- Left: the instances, with a SYNTH/FX badge and a dot for the skin (yellow loading, green
  ready, red failed). Selecting one subscribes it, so its display texts refresh at `poll_ms`.
- Centre: the selected instance. With a skin, the page tabs (one per F-key, nested pages as
  `F1.1`) and the page drawn as the device draws it: background and images, filmstrip knobs at
  the value's frame, button groups with the chosen option lit, labels in the skin's font,
  colour and justification, popups and mode panels shown through `IndexedEnabling`. A knob
  drags vertically (its own height is the full range), a button sets its option, a toggle
  flips, all through `set`. Without a skin, the generic panel: a card per parameter with a knob
  arc for numeric values or the text and a chip per option the app has seen, each card
  draggable.
- Transport bar (once the addin sends `transport`): Play, Stop, Rec, Continue, the state, tempo,
  bar.beat.tick and the source, plus a button asking for a project snapshot.
- Right (once `project` or `midi_in` arrives): the project's tracks with mute/solo, volume and
  pan, and the MIDI events from the addin's port.

## Crates

- `crates/commander-protocol`: serde types for every message in `docs/PROTOCOL.md`, with JSON
  fixtures under `tests/fixtures/`.
- `crates/commander-skin`: parses `TUI.json` (the subset in `docs/SKIN.md`), resolves placed
  instances, handle maps and visibility, and produces a page's draw list and control list from
  live values. No network, no GPU; the Chordsmith skin under `tests/fixtures/chordsmith/` is
  its test subject.
- `crates/commander`: the app: tokio network task (WebSocket with reconnect, HTTP skin fetch
  and cache), the egui UI, and the winit/wgpu window. `--once` builds the app and renders one
  frame without a window.

Fonts: Titillium Web (Regular, SemiBold, Bold; SIL OFL) and Roboto Bold (Apache-2.0) are
bundled in `crates/commander/assets/fonts/`; see `NOTICE.txt` there.
