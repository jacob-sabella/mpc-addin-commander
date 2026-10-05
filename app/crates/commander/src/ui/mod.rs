//! The egui user interface: the connection bar, the instance list, the selected instance's
//! panel (its skin, or the generic panel), the DAW views and the log pane.

pub mod daw;
pub mod fonts;
pub mod generic;
pub mod panel;
pub mod theme;

use crate::config::Config;
use crate::model::{is_stock, ConnState, Instance, Shared, SkinBundle, SkinState, Snapshot};
use crate::net::{Cmd, CmdTx};
use commander_protocol::ClientMessage;
use egui::{Color32, ColorImage, RichText, TextureHandle, TextureId, TextureOptions};
use std::collections::HashMap;
use std::sync::Arc;

/// A knob drag in progress on a skin panel.
#[derive(Debug, Clone)]
pub struct Drag {
    pub id: u32,
    pub param: u32,
    pub start_value: f32,
    pub start_y: f32,
    /// Screen pixels per unit of value.
    pub px_per_unit: f32,
    pub last_sent: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TexKey {
    uid: String,
    file: String,
    frame: Option<u32>,
}

pub struct App {
    pub model: Shared,
    cmd: CmdTx,
    pub config: Config,
    host_field: String,
    port_field: String,
    pub selected: Option<u32>,
    /// The page shown per instance id.
    pages: HashMap<u32, usize>,
    pub drag: Option<Drag>,
    textures: HashMap<TexKey, TextureHandle>,
    show_tracks: bool,
    show_midi: bool,
    pub show_keys: bool,
    pub keys: daw::Keys,
    /// The Sync prompt is open: the Save screen is up on the MPC.
    syncing: bool,
}

/// The Save button on the control surface (note 0x2A on its first cable), pressed and released.
const SAVE_PRESS: [u8; 3] = [0x90, 0x2A, 0x7F];
const SAVE_RELEASE: [u8; 3] = [0x90, 0x2A, 0x00];

impl App {
    pub fn new(model: Shared, cmd: CmdTx, config: Config) -> Self {
        let app = App {
            host_field: config.host.clone(),
            port_field: config.port.to_string(),
            model,
            cmd,
            config,
            selected: None,
            pages: HashMap::new(),
            drag: None,
            textures: HashMap::new(),
            show_tracks: true,
            show_midi: true,
            show_keys: false,
            keys: daw::Keys::default(),
            syncing: false,
        };
        if app.config.connect_on_start && !app.config.host.is_empty() {
            app.connect();
        }
        app
    }

    pub fn send(&self, m: ClientMessage) {
        let _ = self.cmd.send(Cmd::Send(m));
    }

    /// Sends `set` and shows the value at once. A stock plugin's values are read-only.
    pub fn set(&mut self, id: u32, i: u32, value: f32) {
        if is_stock(id) {
            return;
        }
        self.model.lock().unwrap().set_local(id, i, value);
        self.send(ClientMessage::Set { id, i, value });
    }

    fn connect(&self) {
        let _ = self.cmd.send(Cmd::Connect {
            host: self.config.host.clone(),
            port: self.config.port,
        });
    }

    fn apply_fields(&mut self) {
        self.config.host = self.host_field.trim().to_string();
        self.config.port = self
            .port_field
            .trim()
            .parse()
            .unwrap_or(commander_protocol::DEFAULT_PORT);
        self.port_field = self.config.port.to_string();
        if let Err(e) = self.config.save() {
            log::warn!("config: {e:#}");
        }
    }

    /// Remembers the window size for the next start.
    pub fn save_window(&mut self, width: f32, height: f32) {
        self.config.window.width = width;
        self.config.window.height = height;
        if let Err(e) = self.config.save() {
            log::warn!("config: {e:#}");
        }
    }

    pub fn page(&self, id: u32) -> usize {
        self.pages.get(&id).copied().unwrap_or(0)
    }

    pub fn set_page(&mut self, id: u32, page: usize) {
        self.pages.insert(id, page);
    }

    /// A skin image as a GPU texture, uploaded on first use; `frame` picks one frame of a
    /// vertical filmstrip so a strip taller than the GPU's texture limit still draws.
    pub fn texture(
        &mut self,
        ctx: &egui::Context,
        uid: &str,
        file: &str,
        frame: Option<(u32, u32)>,
        bundle: &SkinBundle,
    ) -> Option<TextureId> {
        let key = TexKey {
            uid: uid.to_string(),
            file: file.to_string(),
            frame: frame.map(|f| f.0),
        };
        if let Some(t) = self.textures.get(&key) {
            return Some(t.id());
        }
        let img = bundle.images.get(file)?;
        let (w, h) = (img.width as usize, img.height as usize);
        if w == 0 || h == 0 {
            return None;
        }
        let color = match frame {
            None => ColorImage::from_rgba_unmultiplied([w, h], &img.rgba),
            Some((frame, frames)) => {
                let frames = frames.max(1) as usize;
                let fh = if w * frames == h {
                    w
                } else {
                    (h / frames).max(1)
                };
                let y0 = ((frame as usize).min(frames - 1) * fh).min(h - 1);
                let y1 = (y0 + fh).min(h);
                ColorImage::from_rgba_unmultiplied([w, y1 - y0], &img.rgba[y0 * w * 4..y1 * w * 4])
            }
        };
        let name = format!("{uid}/{file}#{}", frame.map_or(0, |f| f.0));
        let handle = ctx.load_texture(name, color, TextureOptions::LINEAR);
        let id = handle.id();
        self.textures.insert(key, handle);
        Some(id)
    }

    /// One frame of UI, laid out inside the root `Ui` of `Context::run_ui`.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let snap = self.model.lock().unwrap().snapshot();
        egui::Panel::top("connection")
            .frame(egui::Frame::new().fill(theme::MANTLE).inner_margin(8.0))
            .show_inside(ui, |ui| self.connection_bar(ui, &snap));
        if snap.transport.is_some() {
            egui::Panel::top("transport")
                .frame(egui::Frame::new().fill(theme::CRUST).inner_margin(8.0))
                .show_inside(ui, |ui| {
                    daw::transport_bar(self, ui, &snap);
                    ui.add_space(4.0);
                    daw::timeline(self, ui, &snap);
                });
        }
        egui::Panel::bottom("log")
            .resizable(true)
            .default_size(140.0)
            .frame(egui::Frame::new().fill(theme::MANTLE).inner_margin(8.0))
            .show_inside(ui, |ui| self.log_pane(ui, &snap));
        if self.show_keys && snap.transport.is_some() {
            egui::Panel::bottom("keys")
                .frame(egui::Frame::new().fill(theme::CRUST).inner_margin(8.0))
                .show_inside(ui, |ui| daw::keyboard(self, ui, &snap));
        }
        egui::Panel::left("instances")
            .default_size(250.0)
            .frame(egui::Frame::new().fill(theme::MANTLE).inner_margin(8.0))
            .show_inside(ui, |ui| self.instance_list(ui, &snap));
        if snap.project.is_some() || !snap.midi_in.is_empty() {
            egui::Panel::right("daw")
                .default_size(300.0)
                .frame(egui::Frame::new().fill(theme::MANTLE).inner_margin(8.0))
                .show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        if snap.project.is_some() {
                            ui.toggle_value(&mut self.show_tracks, "Tracks");
                        }
                        if !snap.midi_in.is_empty() {
                            ui.toggle_value(&mut self.show_midi, "MIDI in");
                        }
                    });
                    ui.separator();
                    if self.show_tracks {
                        daw::track_list(ui, &snap);
                    }
                    if self.show_midi {
                        daw::midi_pane(ui, &snap);
                    }
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::BASE).inner_margin(8.0))
            .show_inside(ui, |ui| self.centre(ui, &snap));
        let ctx = ui.ctx().clone();
        self.sync_prompt(&ctx);
    }

    fn connection_bar(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Commander").strong().color(theme::MAUVE));
            ui.separator();
            ui.label("Host");
            let host = ui.add(
                egui::TextEdit::singleline(&mut self.host_field)
                    .hint_text("mpc.local")
                    .desired_width(180.0),
            );
            ui.label("Port");
            let port = ui.add(egui::TextEdit::singleline(&mut self.port_field).desired_width(50.0));
            let enter = (host.lost_focus() || port.lost_focus())
                && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let busy = !snap.conn.is_idle();
            if busy && ui.button("Disconnect").clicked() {
                let _ = self.cmd.send(Cmd::Disconnect);
            }
            let can_connect = !self.host_field.trim().is_empty();
            if ui
                .add_enabled(can_connect, egui::Button::new("Connect"))
                .clicked()
                || (enter && can_connect)
            {
                self.apply_fields();
                self.connect();
            }
            ui.separator();
            let (text, colour) = match &snap.conn {
                ConnState::Disconnected => ("Disconnected".to_string(), theme::SUBTEXT),
                ConnState::Connecting {
                    host,
                    port,
                    attempt,
                } => (
                    format!("Connecting to {host}:{port} ({attempt})"),
                    theme::YELLOW,
                ),
                ConnState::Connected { host, port } => {
                    (format!("Connected to {host}:{port}"), theme::GREEN)
                }
            };
            ui.label(RichText::new(text).color(colour));
            if let Some(ms) = snap.latency_ms {
                ui.label(RichText::new(format!("{ms:.1} ms")).color(theme::TEAL));
            }
            if let Some(h) = &snap.hello {
                ui.label(
                    RichText::new(format!(
                        "addin {} · {} · software {}",
                        h.addin, h.device.model, h.device.mpc
                    ))
                    .color(theme::SUBTEXT),
                );
            }
        });
    }

    /// Sync: presses Save on the MPC, then waits for the user to save the project there.
    fn start_sync(&mut self) {
        self.send(ClientMessage::Surface {
            bytes: SAVE_PRESS.to_vec(),
        });
        self.send(ClientMessage::Surface {
            bytes: SAVE_RELEASE.to_vec(),
        });
        self.syncing = true;
    }

    /// The prompt Sync leaves open: OK reads the project file again.
    fn sync_prompt(&mut self, ctx: &egui::Context) {
        if !self.syncing {
            return;
        }
        egui::Window::new("Sync")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label("The Save screen is open on the MPC.");
                ui.label("Tap Project there to save it, then press OK.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("OK").clicked() {
                        self.send(ClientMessage::Project);
                        self.syncing = false;
                    }
                    if ui.button("Cancel").clicked() {
                        self.syncing = false;
                    }
                });
            });
    }

    fn instance_list(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Instances").strong());
            if ui
                .add_enabled(snap.conn.is_connected(), egui::Button::new("Refresh"))
                .clicked()
            {
                self.send(ClientMessage::List);
            }
            if ui
                .add_enabled(snap.conn.is_connected(), egui::Button::new("Sync"))
                .on_hover_text("Save the project on the MPC and read Akai's plugins from it again")
                .clicked()
            {
                self.start_sync();
            }
        });
        ui.separator();
        if snap.instances.is_empty() && snap.stock.is_empty() {
            ui.label(RichText::new("No plugin instances").color(theme::SUBTEXT));
        }
        self.select_by_keys(ui, snap);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (k, inst) in snap.instances.iter().chain(&snap.stock).enumerate() {
                let p = &inst.plugin;
                let stock = is_stock(p.id);
                if stock && k == snap.instances.len() {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Akai plugins, as last saved")
                            .color(theme::SUBTEXT)
                            .size(12.0),
                    );
                }
                let selected = self.selected == Some(p.id);
                let (kind, kind_colour) = match (stock, p.synth) {
                    (true, _) => ("SAVED", theme::OVERLAY),
                    (false, true) => ("SYNTH", theme::BLUE),
                    (false, false) => ("FX", theme::PEACH),
                };
                let text = match &inst.track {
                    Some(t) => format!("{}\n{}", p.name, t),
                    None => format!("{}\n{}", p.name, p.vendor),
                };
                let resp = ui.add_sized(
                    [ui.available_width(), 40.0],
                    egui::Button::selectable(selected, text),
                );
                let badge = egui::Rect::from_min_size(
                    egui::pos2(resp.rect.right() - 54.0, resp.rect.top() + 4.0),
                    egui::vec2(50.0, 16.0),
                );
                ui.painter()
                    .rect_filled(badge, 3.0, kind_colour.gamma_multiply(0.25));
                ui.painter().text(
                    badge.center(),
                    egui::Align2::CENTER_CENTER,
                    kind,
                    egui::FontId::proportional(11.0),
                    kind_colour,
                );
                if p.skin {
                    let dot = egui::pos2(resp.rect.right() - 8.0, resp.rect.bottom() - 8.0);
                    let colour = match snap.skins.get(&p.uid) {
                        Some(SkinState::Ready(_)) => theme::GREEN,
                        Some(SkinState::Loading { .. }) => theme::YELLOW,
                        Some(SkinState::Failed(_)) => theme::RED,
                        None => theme::OVERLAY,
                    };
                    ui.painter().circle_filled(dot, 4.0, colour);
                }
                if resp.clicked() {
                    self.select(p.id);
                }
            }
        });
    }

    /// Selects an instance; a live one is subscribed to for its display texts.
    fn select(&mut self, id: u32) {
        self.selected = Some(id);
        if !is_stock(id) {
            self.send(ClientMessage::Subscribe { ids: vec![id] });
        }
    }

    /// Up and Down move the selection through the instance list while no text field has focus.
    fn select_by_keys(&mut self, ui: &egui::Ui, snap: &Snapshot) {
        let all: Vec<&Instance> = snap.instances.iter().chain(&snap.stock).collect();
        if all.is_empty() || ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let (up, down) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
            )
        });
        if !up && !down {
            return;
        }
        let last = all.len() - 1;
        let at = self
            .selected
            .and_then(|id| all.iter().position(|i| i.plugin.id == id));
        let next = match (at, down) {
            (None, true) => 0,
            (None, false) => last,
            (Some(n), true) => (n + 1).min(last),
            (Some(n), false) => n.saturating_sub(1),
        };
        let id = all[next].plugin.id;
        if self.selected != Some(id) {
            self.select(id);
        }
    }

    fn log_pane(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        ui.label(RichText::new("Log").strong());
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for line in &snap.log {
                    ui.label(RichText::new(line).monospace().size(12.0));
                }
            });
    }

    fn centre(&mut self, ui: &mut egui::Ui, snap: &Snapshot) {
        let Some(inst) = self.selected.and_then(|id| snap.instance(id)) else {
            ui.centered_and_justified(|ui| {
                let text = if snap.conn.is_connected() {
                    "Select an instance"
                } else {
                    "Connect to the addin to see its plugin instances"
                };
                ui.label(RichText::new(text).color(theme::SUBTEXT).size(18.0));
            });
            return;
        };
        let p = &inst.plugin;
        ui.horizontal(|ui| {
            ui.label(RichText::new(&p.name).strong().size(18.0));
            ui.label(RichText::new(&p.vendor).color(theme::SUBTEXT));
            if is_stock(p.id) {
                let mut what = vec!["values as last saved, read-only".to_string()];
                if let Some(t) = &inst.track {
                    what.insert(0, t.clone());
                }
                if !p.product.is_empty() {
                    what.push(format!("preset {}", p.product));
                }
                ui.label(RichText::new(what.join(" · ")).color(theme::OVERLAY));
            } else {
                ui.label(RichText::new(format!("#{} · uid {}", p.id, p.uid)).color(theme::OVERLAY));
            }
        });
        if is_stock(p.id) && p.params.is_empty() {
            ui.label(
                RichText::new("This plugin's saved state is in a format the app can't read yet")
                    .color(theme::SUBTEXT),
            );
            return;
        }
        let skin: Option<Arc<SkinBundle>> = match snap.skins.get(&p.uid) {
            Some(SkinState::Ready(b)) => Some(b.clone()),
            Some(SkinState::Loading { done, total }) => {
                ui.label(
                    RichText::new(format!("Loading the skin: {done} of {total} images"))
                        .color(theme::YELLOW),
                );
                None
            }
            Some(SkinState::Failed(e)) => {
                ui.label(RichText::new(format!("Skin unavailable: {e}")).color(theme::RED));
                None
            }
            None => None,
        };
        match skin {
            Some(bundle) => {
                let page = self
                    .page(p.id)
                    .min(bundle.skin.pages().len().saturating_sub(1));
                let mut chosen = page;
                ui.horizontal(|ui| {
                    for (i, pg) in bundle.skin.pages().iter().enumerate() {
                        let label = if pg.sub_index == 0 {
                            format!("F{} {}", pg.fn_key + 1, pg.name)
                        } else {
                            format!("F{}.{} {}", pg.fn_key + 1, pg.sub_index, pg.name)
                        };
                        ui.selectable_value(&mut chosen, i, label);
                    }
                });
                if chosen != page {
                    self.set_page(p.id, chosen);
                }
                ui.separator();
                let ctx = ui.ctx().clone();
                panel::show(self, &ctx, ui, inst, &bundle, chosen);
            }
            None => generic::show(self, ui, inst),
        }
    }
}

/// egui's straight-alpha colour from a skin colour.
pub fn colour(c: commander_skin::Colour) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// Renders one frame without a window and returns how much it drew: the CI smoke test.
pub fn run_once(mut app: App) -> anyhow::Result<(usize, usize)> {
    let ctx = egui::Context::default();
    fonts::install(&ctx);
    theme::apply(&ctx);
    let size = egui::vec2(app.config.window.width, app.config.window.height);
    let mut raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
        max_texture_side: Some(8192),
        ..Default::default()
    };
    raw.viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .native_pixels_per_point = Some(1.0);
    let out = ctx.run_ui(raw, |ui| app.ui(ui));
    let shapes = out.shapes.len();
    let prims = ctx.tessellate(out.shapes, 1.0).len();
    Ok((shapes, prims))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Image, Model};
    use commander_protocol::{Param, Plugin, ServerMessage, Transport};
    use std::path::PathBuf;
    use std::sync::Mutex;

    fn fixture_bundle() -> SkinBundle {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../commander-skin/tests/fixtures/chordsmith");
        let skin =
            commander_skin::Skin::parse(&std::fs::read_to_string(dir.join("TUI.json")).unwrap())
                .unwrap();
        let mut images = HashMap::new();
        for f in skin.image_files() {
            let bytes = std::fs::read(dir.join(&f)).unwrap();
            let img = crate::net::skin::decode_png(&bytes).unwrap();
            images.insert(f, Arc::new(img));
        }
        SkinBundle { skin, images }
    }

    fn app_with(model: Model) -> App {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let config = Config {
            connect_on_start: false,
            ..Default::default()
        };
        App::new(Arc::new(Mutex::new(model)), tx, config)
    }

    #[test]
    fn renders_empty_and_populated_frames() {
        let (empty, _) = run_once(app_with(Model::new())).unwrap();
        assert!(empty > 5);

        let mut m = Model::new();
        m.conn = ConnState::Connected {
            host: "h".into(),
            port: 1,
        };
        let params: Vec<Param> = (0..80)
            .map(|i| Param {
                i,
                name: format!("Param {i}"),
                value: 0.3,
                text: if i % 3 == 0 {
                    "Minor".into()
                } else {
                    "12.5 ms".into()
                },
                ..Default::default()
            })
            .collect();
        m.apply(ServerMessage::Plugins {
            plugins: vec![
                Plugin {
                    id: 3,
                    name: "Chordsmith".into(),
                    uid: "4368536d".into(),
                    skin: true,
                    synth: true,
                    params: params.clone(),
                    ..Default::default()
                },
                Plugin {
                    id: 4,
                    name: "Plain".into(),
                    params,
                    ..Default::default()
                },
            ],
        });
        m.skins.insert(
            "4368536d".into(),
            SkinState::Ready(Arc::new(fixture_bundle())),
        );
        m.apply(ServerMessage::Transport(Transport {
            playing: true,
            tempo: Some(120.0),
            ..Default::default()
        }));
        let mut app = app_with(m);
        app.selected = Some(3);
        let (skinned, prims) = run_once_keep(&mut app);
        assert!(skinned > empty + 40, "{skinned} shapes, {prims} primitives");
        let first = app.textures.len();
        assert!(first > 8, "the panel uploaded {first} textures");
        // Another page uploads its own images.
        app.set_page(3, 1);
        run_once_keep(&mut app);
        assert!(app.textures.len() > first);
        app.selected = Some(4);
        let (generic, _) = run_once_keep(&mut app);
        assert!(generic > empty + 40, "{generic} shapes");
    }

    #[test]
    fn stock_plugins_draw_read_only_and_sync_presses_save() {
        let fixture = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../commander-protocol/tests/fixtures/server_project.json"
        ))
        .unwrap();
        let mut m = Model::new();
        m.conn = ConnState::Connected {
            host: "h".into(),
            port: 1,
        };
        m.apply(serde_json::from_str(&fixture).unwrap());
        let uid = m.stock[0].plugin.uid.clone();
        // A one-fader skin with a 10 x 10 thumb.
        let skin = commander_skin::Skin::parse(
            r#"{"pageData": {
              "tabs": [{"tabName": "Main", "componentName": "page", "initialSize": "0 0 400 300"}],
              "componentDefinitions": {"localComponentDefinitions": [
                {"key": "page", "value": {"componentsData": [
                  {"componentData": {"name": "Fader", "type": "Slider",
                     "data": {"direction": "Vertical", "thumbImage": "thumb.png"}},
                   "handle remapping": {"map": []},
                   "bounds": {"bounds": "10 10 20 200", "whenVisible": "Always"}}]}}]}}}"#,
        )
        .unwrap();
        let mut images = HashMap::new();
        images.insert(
            "thumb.png".to_string(),
            Arc::new(Image {
                width: 10,
                height: 10,
                rgba: vec![255; 400],
            }),
        );
        m.skins
            .insert(uid, SkinState::Ready(Arc::new(SkinBundle { skin, images })));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            Arc::new(Mutex::new(m)),
            tx,
            Config {
                connect_on_start: false,
                ..Default::default()
            },
        );
        app.select(crate::model::STOCK_ID);
        run_once_keep(&mut app);
        assert!(app.textures.len() == 1, "the thumb is drawn");
        // Read-only: no set, no subscribe.
        app.set(crate::model::STOCK_ID, 0, 1.0);
        assert!(rx.try_recv().is_err());
        app.start_sync();
        let mut sent = Vec::new();
        while let Ok(Cmd::Send(ClientMessage::Surface { bytes })) = rx.try_recv() {
            sent.push(bytes);
        }
        assert_eq!(sent, vec![vec![0x90, 0x2A, 0x7F], vec![0x90, 0x2A, 0x00]]);
        assert!(app.syncing);
        run_once_keep(&mut app);
    }

    fn run_once_keep(app: &mut App) -> (usize, usize) {
        let ctx = egui::Context::default();
        fonts::install(&ctx);
        theme::apply(&ctx);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 860.0),
            )),
            ..Default::default()
        };
        let out = ctx.run_ui(raw, |ui| app.ui(ui));
        let shapes = out.shapes.len();
        (shapes, ctx.tessellate(out.shapes, 1.0).len())
    }

    #[test]
    fn filmstrip_frames_are_cut_from_the_strip() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            Arc::new(Mutex::new(Model::new())),
            tx,
            Config {
                connect_on_start: false,
                ..Default::default()
            },
        );
        let mut rgba = Vec::new();
        for frame in 0..4u8 {
            rgba.extend(std::iter::repeat_n([frame, 0, 0, 255], 4).flatten());
        }
        let mut images = HashMap::new();
        images.insert(
            "strip.png".to_string(),
            Arc::new(Image {
                width: 2,
                height: 8,
                rgba,
            }),
        );
        let bundle = SkinBundle {
            skin: commander_skin::Skin::parse(
                r#"{"pageData": {"tabs": [], "componentDefinitions": {"localComponentDefinitions": []}}}"#,
            )
            .unwrap(),
            images,
        };
        let ctx = egui::Context::default();
        let a = app
            .texture(&ctx, "u", "strip.png", Some((2, 4)), &bundle)
            .unwrap();
        let again = app
            .texture(&ctx, "u", "strip.png", Some((2, 4)), &bundle)
            .unwrap();
        assert_eq!(a, again);
        assert_eq!(app.textures.len(), 1);
        let whole = app.texture(&ctx, "u", "strip.png", None, &bundle).unwrap();
        assert_ne!(a, whole);
        assert!(app
            .texture(&ctx, "u", "missing.png", None, &bundle)
            .is_none());
    }
}
