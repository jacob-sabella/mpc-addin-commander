# mpc-addin-commander

Dock an Akai MPC OS standalone device (MPC Live/One/X/Key, Force) to a computer: the plugins MPC has loaded appear in
a native desktop app, drawn with their own skins, and follow and drive the device live. MPC's transport (play,
stop, record, tempo, position) and the project's tracks are there too, and the app plays MIDI into MPC.

Two halves:

- **The addin** (`src/`, C, armhf): a small shared library MPC loads at its start through `LD_PRELOAD`, running inside
  the MPC process. It sees every VST2 plugin instance MPC creates (through the `dlsym` call MPC makes to load a
  plugin), polls their parameters, and serves them with the plugins' own skin files over HTTP and a WebSocket on
  port 6730. It opens an ALSA sequencer port, `MPC Commander`, that MPC treats like any MIDI device: MPC's clock,
  start/stop and MMC come in over it and become transport state, and the app's MIDI and transport commands go out
  over it into MPC. A project snapshot comes from the most recent project file. Nothing in MPC is patched; Akai's
  own plugins run inside MPC where the addin can't see them live, so the snapshot carries their saved state and
  the addin serves their skins from the device.
- **The desktop app** (`app/`, Rust + egui): connects to the device over Wi-Fi or Ethernet (through an SSH tunnel), renders each plugin from
  its `TUI.json` and PNGs with the live values (or a generic knob panel when a plugin ships no skin), and sends the
  changes back. Around the panels: a transport bar (play, stop, record, continue, bar steps), a timeline of the
  current sequence with its loop and the playhead (click a bar to locate there), the project's tracks (colour,
  type, record arm, mute and solo, volume and pan, plugin and preset), an on-screen keyboard (mouse or computer
  keys, any channel) and the MIDI arriving from MPC. Up and Down step through the plugin instances. Akai's own
  plugins in the project list below them, drawn with their own skins from the values of the last save. They are
  **read-only** (a READ-ONLY badge, banner and tag; a click or drag sends nothing): change them on the MPC, then
  Sync presses Save on the MPC, you tap Project there, and OK reads the saved project again.

**Status:** the addin passes its offline tests (x86: the hook, poll thread, server, transport and project paths under
ASan+UBSan and TSan, the real `.so` preloaded into a process named `MPC`, the sequencer port where the build machine
has one, the installer under BusyBox) and builds for armhf (glibc symbols up to 2.31). On an MPC Key 37 (software
3.9.1.2): plugin instances, live values and edits, the MIDI port, MMC transport both ways and the project snapshot
work, the app runs against it, MPC moves its playhead to a locate from the app, and record from the app puts MPC in
record. MPC ignores transport while its New Project dialog is up, and it forgets Receive MMC across a restart unless
the setting is saved (mpc-vst-plugins `docs/NOTES.md`).

## Install the addin

Download the release zip (`MPC-Commander-addin-<version>-mpc-armv7.zip`), unzip it, and follow its `INSTALL.md`:
copy the folder to the device over SSH and run `install.sh` there. It needs a modded device with root shell access,
and it restarts MPC once. `uninstall.sh` reverses it, and deletes the settings file with it. The addin is also in the
[MPC OS Plugin Catalog](https://sd88me.github.io/mpc-vst-plugins/), so the installer app and `mpc-store.sh` can install
it. After a firmware update, run the installer again; if the addin ever stops MPC from starting, see "When an addin
stops MPC from starting" in mpc-vst-plugins' `docs/ADDINS.md`.

Settings are in `mpc_commander_addin.conf` next to the `.so` (`/data/mpc-addins/commander/`): the listen address
and port, how often parameters are polled, whether skins are served, whether the sequencer port is opened, what a
transport command sends (MMC, MIDI real-time or both), the control-surface injector file, and where the project
snapshot comes from. `enabled=0` keeps the addin idle.

For transport and MIDI, enable the `MPC Commander` port in MPC's MIDI preferences: as an input (with Receive MMC on,
so the app's play/stop/record reach MPC) and as an output with clock sync and Send MMC (so the app follows MPC).

## Connecting: an SSH tunnel

The addin listens on the device only (`bind=127.0.0.1`): it has no login, and it runs as root inside MPC. To reach it
from a computer, open an SSH tunnel and leave it running:

```sh
ssh -N -L 6730:127.0.0.1:6730 root@<device address>
```

Then connect the app to `localhost` (port 6730). To open the addin to your network instead, set `bind=0.0.0.0` in
the settings: anyone who can reach the port can then change the plugins' parameters, send MIDI and transport into
MPC, write to the control-surface injector file and read the project, and a web page open in a browser on that
network can send it requests too.

## Run the app

See `app/README.md`. In short: open the tunnel, `cargo run --release -p commander` from `app/`, enter `localhost`,
Connect.

## Protocol and rendering

`docs/PROTOCOL.md` is the addin's WebSocket and HTTP interface; `docs/SKIN.md` is what the app renders from a plugin's
skin files and how.

## Develop

```sh
tests/test.sh        # the addin's offline tests (x86; needs cc, python3, busybox for the installer test)
./build.sh           # the armhf .so, in Docker (arm32v7/gcc:11-bullseye, glibc <= 2.31)
tools/release.sh 0.1.0   # the release zip, checked as the catalog checks it (needs mpc-vst-plugins next to this repo)
cd app && cargo test && cargo clippy --all-targets -- -D warnings
```

The addin is catalog-conformant (`addin.manifest`, the shared installer from mpc-vst-plugins). Releases are drafted
by the `Release` workflow (the addin, tag `commander-addin-v<version>`) and `App release` (the app, tag `v<version>`).

## License

MIT. The app bundles Titillium Web (SIL Open Font License) and, where found, Roboto (Apache 2.0): see
`app/crates/commander/assets/fonts/`.
