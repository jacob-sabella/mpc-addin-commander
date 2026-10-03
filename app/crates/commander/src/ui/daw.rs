//! The DAW views the addin feeds later: a transport bar, the project's track list and the
//! MIDI events from the addin's port. Each draws only once its message has arrived.

use super::{theme, App};
use crate::model::Snapshot;
use commander_protocol::{ClientMessage, TransportCmd, TransportSource};
use egui::{RichText, Stroke, StrokeKind};

pub fn transport_bar(app: &mut App, ui: &mut egui::Ui, snap: &Snapshot) {
    let Some(t) = &snap.transport else {
        return;
    };
    ui.horizontal(|ui| {
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
    });
}

pub fn track_list(ui: &mut egui::Ui, snap: &Snapshot) {
    let Some(p) = &snap.project else {
        return;
    };
    ui.label(RichText::new(&p.name).strong().size(16.0));
    ui.label(RichText::new(&p.path).color(theme::OVERLAY).size(11.0));
    if let Some(t) = p.tempo {
        ui.label(RichText::new(format!("{t:.1} BPM")).color(theme::SUBTEXT));
    }
    ui.separator();
    egui::ScrollArea::vertical()
        .id_salt("tracks")
        .max_height(ui.available_height() * 0.6)
        .show(ui, |ui| {
            for tr in &p.tracks {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{:>2}", tr.index + 1)).monospace());
                    ui.label(RichText::new(&tr.name).strong());
                    ui.label(RichText::new(&tr.kind).color(theme::SUBTEXT).size(11.0));
                    if tr.mute {
                        ui.label(RichText::new("M").strong().color(theme::YELLOW));
                    }
                    if tr.solo {
                        ui.label(RichText::new("S").strong().color(theme::GREEN));
                    }
                });
                ui.horizontal(|ui| {
                    bar(ui, "vol", tr.volume, theme::TEAL);
                    bar(ui, "pan", tr.pan, theme::BLUE);
                    if let Some(plugin) = &tr.plugin {
                        ui.label(RichText::new(plugin).color(theme::MAUVE).size(11.0));
                    }
                });
                ui.add_space(4.0);
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
