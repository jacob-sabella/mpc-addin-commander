//! The DAW views: the transport bar, the sequence timeline, the project's track list, an
//! on-screen keyboard and the MIDI events from the addin's port. Each draws only once its
//! message has arrived.

use super::{theme, App};
use crate::midi;
use crate::model::Snapshot;
use commander_protocol::{ClientMessage, TransportCmd, TransportSource};
use egui::{RichText, Stroke, StrokeKind};
use std::collections::BTreeSet;

/// Ticks per beat in the addin's `transport` position.
const TICKS: f32 = 960.0;

/// The keyboard's settings and the notes it holds.
pub struct Keys {
    /// The octave of the keyboard's first C (`C3` = 48).
    pub octave: i8,
    /// 0..15.
    pub channel: u8,
    pub velocity: u8,
    /// Play from the computer keyboard (A W S E D F T G Y H U J K O L P).
    pub computer: bool,
    mouse: Option<u8>,
    held: BTreeSet<u8>,
}

impl Default for Keys {
    fn default() -> Self {
        Keys {
            octave: 3,
            channel: 0,
            velocity: 100,
            computer: false,
            mouse: None,
            held: BTreeSet::new(),
        }
    }
}

impl Keys {
    fn base(&self) -> u8 {
        ((i16::from(self.octave) + 1) * 12).clamp(0, 108) as u8
    }
}

/// The computer keys that play notes, as semitones above the keyboard's first C.
const COMPUTER_KEYS: [(egui::Key, u8); 16] = [
    (egui::Key::A, 0),
    (egui::Key::W, 1),
    (egui::Key::S, 2),
    (egui::Key::E, 3),
    (egui::Key::D, 4),
    (egui::Key::F, 5),
    (egui::Key::T, 6),
    (egui::Key::G, 7),
    (egui::Key::Y, 8),
    (egui::Key::H, 9),
    (egui::Key::U, 10),
    (egui::Key::J, 11),
    (egui::Key::K, 12),
    (egui::Key::O, 13),
    (egui::Key::L, 14),
    (egui::Key::P, 15),
];

/// The current sequence's beats per bar, 4 when unknown.
fn beats_per_bar(snap: &Snapshot) -> u32 {
    snap.project
        .as_ref()
        .and_then(|p| p.sequence.as_ref())
        .and_then(|s| s.beats_per_bar)
        .filter(|n| *n > 0)
        .unwrap_or(4)
}

/// The tempo to turn bars into time code: the clock's, else the project's.
fn tempo(snap: &Snapshot) -> Option<f32> {
    snap.transport
        .as_ref()
        .and_then(|t| t.tempo)
        .or_else(|| snap.project.as_ref().and_then(|p| p.tempo))
        .filter(|t| *t > 0.0)
}

/// The playhead in bars from the sequence start (0.0 = bar 1).
fn position(snap: &Snapshot) -> Option<f32> {
    let t = snap.transport.as_ref()?;
    let bar = t.bar?;
    let beat = t.beat.unwrap_or(1);
    let tick = t.tick.unwrap_or(0) as f32;
    let bpb = beats_per_bar(snap) as f32;
    Some(bar.saturating_sub(1) as f32 + (beat.saturating_sub(1) as f32 + tick / TICKS) / bpb)
}

/// MPC counts on through a looping sequence: fold the position back into the loop.
fn wrap(pos: f32, s: &commander_protocol::Sequence) -> f32 {
    let (start, end) = (s.loop_start as f32, s.loop_end as f32);
    if s.r#loop && end > start && pos >= end {
        start + (pos - start) % (end - start)
    } else {
        pos
    }
}

/// Sends an MMC locate to the start of `bar` (0-based).
fn locate(app: &mut App, snap: &Snapshot, bar: u32) {
    let Some(tempo) = tempo(snap) else {
        return;
    };
    let bytes = midi::locate(bar, beats_per_bar(snap), tempo);
    app.send(ClientMessage::Midi { bytes });
}

pub fn transport_bar(app: &mut App, ui: &mut egui::Ui, snap: &Snapshot) {
    let Some(t) = &snap.transport else {
        return;
    };
    ui.horizontal(|ui| {
        let can_locate = tempo(snap).is_some() && !t.playing;
        let here = t.bar.unwrap_or(1).saturating_sub(1);
        let mut to = None;
        let hint = "MMC locate; needs a tempo and a stopped transport";
        if ui
            .add_enabled(can_locate, egui::Button::new("|<"))
            .on_hover_text(hint)
            .clicked()
        {
            to = Some(0);
        }
        if ui
            .add_enabled(can_locate, egui::Button::new("<<"))
            .on_hover_text(hint)
            .clicked()
        {
            // Mid-bar goes back to the bar's start, at a bar's start one bar back.
            let at_start = t.beat.unwrap_or(1) <= 1 && t.tick.unwrap_or(0) == 0;
            to = Some(if at_start {
                here.saturating_sub(1)
            } else {
                here
            });
        }
        if ui
            .add_enabled(can_locate, egui::Button::new(">>"))
            .on_hover_text(hint)
            .clicked()
        {
            to = Some(here + 1);
        }
        if let Some(bar) = to {
            locate(app, snap, bar);
        }
        let mut cmd = None;
        if ui.button("Play").clicked() {
            cmd = Some(TransportCmd::Play);
        }
        if ui.button("Stop").clicked() {
            cmd = Some(TransportCmd::Stop);
        }
        if ui.button("Rec").clicked() {
            cmd = Some(TransportCmd::Record);
        }
        if ui.button("Continue").clicked() {
            cmd = Some(TransportCmd::Continue);
        }
        if let Some(cmd) = cmd {
            app.send(ClientMessage::Transport { cmd });
        }
        ui.separator();
        let state = match (t.playing, t.recording) {
            (true, true) => ("RECORDING", theme::RED),
            (true, false) => ("PLAYING", theme::GREEN),
            (false, true) => ("REC ARMED", theme::PEACH),
            (false, false) => ("STOPPED", theme::SUBTEXT),
        };
        ui.label(RichText::new(state.0).strong().color(state.1));
        let tempo = t.tempo.map_or("--".to_string(), |b| format!("{b:.1}"));
        ui.label(RichText::new(format!("{tempo} BPM")).monospace().size(16.0));
        let pos = |v: Option<u32>| v.map_or("--".to_string(), |n| format!("{n:02}"));
        ui.label(
            RichText::new(format!(
                "{}.{}.{}",
                pos(t.bar),
                pos(t.beat),
                t.tick.map_or("---".to_string(), |n| format!("{n:03}"))
            ))
            .monospace()
            .size(16.0)
            .color(theme::LAVENDER),
        );
        let source = match t.source {
            TransportSource::Mmc => "MMC",
            TransportSource::Clock => "clock",
            TransportSource::Project => "project",
            TransportSource::Unknown => "?",
        };
        ui.label(RichText::new(format!("source: {source}")).color(theme::SUBTEXT));
        ui.separator();
        if ui.button("Project snapshot").clicked() {
            app.send(ClientMessage::Project);
        }
        ui.toggle_value(&mut app.show_keys, "Keys");
    });
}

/// Bars along the width, the sequence's loop shaded, the playhead, and a click to locate.
pub fn timeline(app: &mut App, ui: &mut egui::Ui, snap: &Snapshot) {
    let seq = snap.project.as_ref().and_then(|p| p.sequence.as_ref());
    let pos = position(snap).map(|p| match seq {
        Some(s) => wrap(p, s),
        None => p,
    });
    let bars = seq
        .map(|s| s.bars)
        .filter(|n| *n > 0)
        .unwrap_or_else(|| (pos.unwrap_or(0.0) as u32 + 4).max(16));
    ui.horizontal(|ui| match seq {
        Some(s) => {
            ui.label(RichText::new(&s.name).strong());
            ui.label(RichText::new(format!("{bars} bars")).color(theme::SUBTEXT));
            if s.r#loop {
                ui.label(
                    RichText::new(format!("loop {}-{}", s.loop_start + 1, s.loop_end))
                        .color(theme::GREEN),
                );
            }
        }
        None => {
            ui.label(RichText::new("Timeline").strong());
            ui.label(
                RichText::new("take a project snapshot for the sequence's length and loop")
                    .color(theme::OVERLAY),
            );
        }
    });
    let width = ui.available_width();
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(width, 34.0), egui::Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect(
        rect,
        3.0,
        theme::SURFACE0,
        Stroke::new(1.0_f32, theme::SURFACE1),
        StrokeKind::Inside,
    );
    let per_bar = rect.width() / bars as f32;
    let x_of = |bar: f32| rect.left() + bar.clamp(0.0, bars as f32) * per_bar;
    if let Some(s) = seq.filter(|s| s.r#loop && s.loop_end > s.loop_start) {
        let r = egui::Rect::from_x_y_ranges(
            x_of(s.loop_start as f32)..=x_of(s.loop_end as f32),
            rect.top()..=rect.top() + 8.0,
        );
        painter.rect_filled(r, 2.0, theme::GREEN.gamma_multiply(0.6));
    }
    let step = [1u32, 2, 4, 8, 16, 32, 64]
        .into_iter()
        .find(|n| per_bar * *n as f32 >= 26.0)
        .unwrap_or(128);
    for bar in 0..=bars {
        let x = x_of(bar as f32);
        let major = bar % step == 0;
        let top = if major {
            rect.top() + 10.0
        } else {
            rect.bottom() - 8.0
        };
        painter.line_segment(
            [egui::pos2(x, top), egui::pos2(x, rect.bottom())],
            Stroke::new(
                1.0_f32,
                if major {
                    theme::SURFACE2
                } else {
                    theme::SURFACE1
                },
            ),
        );
        if major && bar < bars {
            painter.text(
                egui::pos2(x + 3.0, rect.top() + 10.0),
                egui::Align2::LEFT_TOP,
                (bar + 1).to_string(),
                egui::FontId::monospace(11.0),
                theme::SUBTEXT,
            );
        }
    }
    if let Some(p) = pos {
        let x = x_of(p);
        painter.line_segment(
            [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
            Stroke::new(2.0_f32, theme::LAVENDER),
        );
    }
    let can_locate = tempo(snap).is_some() && !snap.transport.as_ref().is_some_and(|t| t.playing);
    if let Some(at) = resp.hover_pos() {
        let bar = (((at.x - rect.left()) / per_bar) as u32).min(bars - 1);
        let tip = if can_locate {
            format!("click: locate to bar {}", bar + 1)
        } else {
            format!(
                "bar {} (locate needs a tempo and a stopped transport)",
                bar + 1
            )
        };
        resp.clone().on_hover_text(tip);
    }
    if resp.clicked() && can_locate {
        if let Some(at) = resp.interact_pointer_pos() {
            let bar = (((at.x - rect.left()) / per_bar) as u32).min(bars - 1);
            locate(app, snap, bar);
        }
    }
}

/// Two octaves and a C, played with the mouse or the computer keyboard, out of the addin's
/// port on the chosen channel. MPC routes them like any MIDI input (to the selected track
/// when the port's Track input is on).
pub fn keyboard(app: &mut App, ui: &mut egui::Ui, snap: &Snapshot) {
    let connected = snap.conn.is_connected();
    let mut out: Vec<Vec<u8>> = Vec::new();
    let keys = &mut app.keys;
    let release_all = |keys: &mut Keys, out: &mut Vec<Vec<u8>>| {
        for n in std::mem::take(&mut keys.held) {
            out.push(midi::note_off(keys.channel, n));
        }
        keys.mouse = None;
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new("Keys").strong());
        ui.label("Ch");
        let mut ch = keys.channel + 1;
        if ui
            .add(egui::DragValue::new(&mut ch).range(1..=16))
            .changed()
        {
            release_all(keys, &mut out);
            keys.channel = ch - 1;
        }
        ui.label("Octave");
        if ui.small_button("-").clicked() && keys.octave > -1 {
            release_all(keys, &mut out);
            keys.octave -= 1;
        }
        ui.label(RichText::new(midi::note_name(keys.base())).monospace());
        if ui.small_button("+").clicked() && keys.octave < 7 {
            release_all(keys, &mut out);
            keys.octave += 1;
        }
        ui.label("Vel");
        ui.add(egui::Slider::new(&mut keys.velocity, 1..=127));
        if ui
            .checkbox(&mut keys.computer, "Computer keys")
            .on_hover_text("A W S E D F T G Y H U J K O L P play; Z and X change the octave")
            .changed()
            && !keys.computer
        {
            release_all(keys, &mut out);
        }
        if ui.button("All notes off").clicked() {
            release_all(keys, &mut out);
            out.push(vec![0xB0 | keys.channel, 123, 0]);
        }
    });

    // The piano: 15 white keys from the base C.
    const WHITE: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
    let base = keys.base();
    let whites: Vec<u8> = (0..15)
        .map(|i| base + 12 * (i / 7) as u8 + WHITE[i % 7])
        .filter(|n| *n <= 127)
        .collect();
    let wk = (ui.available_width() / whites.len() as f32).min(40.0);
    let size = egui::vec2(wk * whites.len() as f32, 90.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let white_rect = |i: usize| {
        egui::Rect::from_min_size(
            egui::pos2(rect.left() + i as f32 * wk, rect.top()),
            egui::vec2(wk - 1.0, rect.height()),
        )
    };
    // A black key sits over the boundary after white key i when one follows that white key.
    let blacks: Vec<(u8, egui::Rect)> = whites
        .iter()
        .enumerate()
        .filter(|(_, n)| matches!(*n % 12, 0 | 2 | 5 | 7 | 9) && **n < 127)
        .filter(|(i, _)| *i + 1 < whites.len())
        .map(|(i, n)| {
            let x = rect.left() + (i + 1) as f32 * wk - wk * 0.3;
            let r = egui::Rect::from_min_size(
                egui::pos2(x, rect.top()),
                egui::vec2(wk * 0.6, rect.height() * 0.6),
            );
            (n + 1, r)
        })
        .collect();
    let note_at = |p: egui::Pos2| -> Option<u8> {
        if let Some((n, _)) = blacks.iter().find(|(_, r)| r.contains(p)) {
            return Some(*n);
        }
        (0..whites.len())
            .find(|i| white_rect(*i).contains(p))
            .map(|i| whites[i])
    };

    // Mouse: press plays, dragging slides, release stops.
    let want = if resp.is_pointer_button_down_on() && connected {
        resp.interact_pointer_pos().and_then(note_at)
    } else {
        None
    };
    if want != keys.mouse {
        if let Some(n) = keys.mouse.take() {
            if keys.held.remove(&n) {
                out.push(midi::note_off(keys.channel, n));
            }
        }
        if let Some(n) = want {
            if keys.held.insert(n) {
                out.push(midi::note_on(keys.channel, n, keys.velocity));
            }
            keys.mouse = Some(n);
        }
    }

    // Computer keys, while no text field has focus.
    if keys.computer && connected && !ui.ctx().egui_wants_keyboard_input() {
        let events = ui.input(|i| i.events.clone());
        for e in events {
            let egui::Event::Key {
                key,
                pressed,
                repeat: false,
                ..
            } = e
            else {
                continue;
            };
            if pressed && (key == egui::Key::Z || key == egui::Key::X) {
                release_all(keys, &mut out);
                keys.octave = (keys.octave + if key == egui::Key::X { 1 } else { -1 }).clamp(-1, 7);
                continue;
            }
            let Some((_, off)) = COMPUTER_KEYS.iter().find(|(k, _)| *k == key) else {
                continue;
            };
            let n = keys.base().saturating_add(*off).min(127);
            if pressed && keys.held.insert(n) {
                out.push(midi::note_on(keys.channel, n, keys.velocity));
            } else if !pressed && keys.held.remove(&n) {
                out.push(midi::note_off(keys.channel, n));
            }
        }
    }

    let painter = ui.painter_at(rect);
    for (i, n) in whites.iter().enumerate() {
        let r = white_rect(i);
        let fill = if keys.held.contains(n) {
            theme::MAUVE
        } else {
            theme::TEXT
        };
        painter.rect_filled(r, 2.0, fill);
        if n % 12 == 0 {
            painter.text(
                egui::pos2(r.center().x, r.bottom() - 4.0),
                egui::Align2::CENTER_BOTTOM,
                midi::note_name(*n),
                egui::FontId::proportional(10.0),
                theme::CRUST,
            );
        }
    }
    for (n, r) in &blacks {
        let fill = if keys.held.contains(n) {
            theme::MAUVE
        } else {
            theme::CRUST
        };
        painter.rect_filled(*r, 2.0, fill);
    }
    for bytes in out {
        app.send(ClientMessage::Midi { bytes });
    }
}

pub fn track_list(ui: &mut egui::Ui, snap: &Snapshot) {
    let Some(p) = &snap.project else {
        return;
    };
    ui.label(RichText::new(&p.name).strong().size(16.0));
    ui.label(RichText::new(&p.path).color(theme::OVERLAY).size(11.0));
    ui.horizontal(|ui| {
        if let Some(t) = p.tempo {
            ui.label(RichText::new(format!("{t:.1} BPM")).color(theme::SUBTEXT));
        }
        if !p.key.is_empty() {
            ui.label(RichText::new(&p.key).color(theme::SUBTEXT));
        }
        ui.label(RichText::new(format!("{} tracks", p.tracks.len())).color(theme::SUBTEXT));
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("tracks")
        .max_height(if snap.midi_in.is_empty() {
            f32::INFINITY
        } else {
            ui.available_height() * 0.6
        })
        .show(ui, |ui| {
            for tr in &p.tracks {
                let current = p.current_track == Some(tr.index);
                let frame = egui::Frame::new()
                    .inner_margin(4.0)
                    .corner_radius(3.0)
                    .fill(if current {
                        theme::SURFACE0
                    } else {
                        egui::Color32::TRANSPARENT
                    });
                frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let (sw, _) =
                            ui.allocate_exact_size(egui::vec2(6.0, 16.0), egui::Sense::hover());
                        let c = tr.colour.unwrap_or(0x585b70);
                        let colour =
                            egui::Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8);
                        ui.painter().rect_filled(sw, 1.0, colour);
                        ui.label(RichText::new(format!("{:>2}", tr.index + 1)).monospace());
                        ui.label(RichText::new(&tr.name).strong());
                        ui.label(
                            RichText::new(&tr.type_name)
                                .color(theme::SUBTEXT)
                                .size(11.0),
                        );
                        if tr.record_arm {
                            ui.label(RichText::new("R").strong().color(theme::RED));
                        }
                        if tr.mute || tr.track_mute {
                            ui.label(RichText::new("M").strong().color(theme::YELLOW));
                        }
                        if tr.solo {
                            ui.label(RichText::new("S").strong().color(theme::GREEN));
                        }
                    });
                    ui.horizontal(|ui| {
                        bar(ui, "vol", tr.volume, theme::TEAL);
                        bar(ui, "pan", tr.pan, theme::BLUE);
                    });
                    if let Some(plugin) = &tr.plugin {
                        let text = if plugin.preset.is_empty() {
                            plugin.name.clone()
                        } else {
                            format!("{} · {}", plugin.name, plugin.preset)
                        };
                        ui.label(RichText::new(text).color(theme::MAUVE).size(11.0))
                            .on_hover_text(format!("{} ({})", plugin.vendor, plugin.format));
                    }
                });
            }
        });
}

fn bar(ui: &mut egui::Ui, label: &str, value: f32, colour: egui::Color32) {
    ui.label(RichText::new(label).size(10.0).color(theme::OVERLAY));
    let (rect, _) = ui.allocate_exact_size(egui::vec2(70.0, 8.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect(
        rect,
        2.0,
        theme::SURFACE0,
        Stroke::new(1.0_f32, theme::SURFACE1),
        StrokeKind::Inside,
    );
    let mut fill = rect;
    fill.set_width(rect.width() * value.clamp(0.0, 1.0));
    painter.rect_filled(fill, 2.0, colour);
}

pub fn midi_pane(ui: &mut egui::Ui, snap: &Snapshot) {
    if snap.midi_in.is_empty() {
        return;
    }
    ui.label(RichText::new("MIDI in").strong());
    egui::ScrollArea::vertical()
        .id_salt("midi")
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for m in &snap.midi_in {
                let hex: Vec<String> = m.bytes.iter().map(|b| format!("{b:02X}")).collect();
                ui.label(
                    RichText::new(format!("{:>10}  {}", m.ms, hex.join(" ")))
                        .monospace()
                        .size(12.0),
                );
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use commander_protocol::Sequence;

    #[test]
    fn wrap_folds_into_the_loop() {
        let s = Sequence {
            bars: 20,
            r#loop: true,
            loop_start: 0,
            loop_end: 20,
            ..Default::default()
        };
        assert_eq!(wrap(5.5, &s), 5.5);
        assert_eq!(wrap(30.75, &s), 10.75);
        let inner = Sequence {
            loop_start: 4,
            loop_end: 8,
            ..s.clone()
        };
        assert_eq!(wrap(9.0, &inner), 5.0);
        assert_eq!(wrap(3.0, &inner), 3.0);
        let off = Sequence { r#loop: false, ..s };
        assert_eq!(wrap(30.0, &off), 30.0);
    }
}
