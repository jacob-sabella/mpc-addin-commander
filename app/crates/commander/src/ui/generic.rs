//! The generic panel for a plugin without a skin (or whose skin has not arrived): a grid of
//! parameter cards. Numeric parameters get a knob arc, option parameters show their text with
//! a chip per option the app has seen so far; every card changes its value by a vertical drag.
//! Compact (per plugin, kept in the config): a short row per parameter with a bar that follows
//! the pointer, or the option chips.

use super::{theme, App};
use crate::model::{is_numeric, Instance};
use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind};

const CARD: egui::Vec2 = egui::Vec2::new(150.0, 128.0);
const ROW_H: f32 = 26.0;
const NAME_W: f32 = 150.0;
const VALUE_W: f32 = 100.0;
const COMPACT_MAX_W: f32 = 760.0;
/// Screen pixels of drag per unit of value.
const DRAG_PX: f32 = 200.0;

pub fn show(app: &mut App, ui: &mut egui::Ui, inst: &Instance) {
    let id = inst.plugin.id;
    let read_only = crate::model::is_stock(id);
    let uid = &inst.plugin.uid;
    let compact = app.config.compact.contains(uid);
    ui.horizontal(|ui| {
        if !read_only {
            let hint = if compact {
                "Generic panel: click or drag a bar to change its value"
            } else {
                "Generic panel: drag a card vertically to change its value"
            };
            ui.label(RichText::new(hint).color(theme::SUBTEXT).size(12.0));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if compact { "Cards" } else { "Compact" };
            if ui
                .button(label)
                .on_hover_text("Show the parameters as cards or as compact rows")
                .clicked()
            {
                if compact {
                    app.config.compact.remove(uid);
                } else {
                    app.config.compact.insert(uid.clone());
                }
                if let Err(e) = app.config.save() {
                    log::warn!("config: {e:#}");
                }
            }
        });
    });
    if compact {
        show_compact(app, ui, inst, read_only);
        return;
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for p in &inst.plugin.params {
                if p.name.ends_with("__open") {
                    continue;
                }
                let options = inst.options.get(&p.i);
                let (rect, resp) = ui.allocate_exact_size(CARD, Sense::click_and_drag());
                let painter = ui.painter_at(rect);
                painter.rect(
                    rect,
                    6.0,
                    theme::SURFACE0,
                    Stroke::new(1.0_f32, theme::SURFACE1),
                    StrokeKind::Inside,
                );
                painter.text(
                    Pos2::new(rect.center().x, rect.top() + 14.0),
                    Align2::CENTER_CENTER,
                    &p.name,
                    FontId::proportional(13.0),
                    theme::TEXT,
                );
                let option_like = !p.text.is_empty() && !is_numeric(&p.text);
                let mut clicked: Option<f32> = None;
                if option_like {
                    painter.text(
                        Pos2::new(rect.center().x, rect.top() + 52.0),
                        Align2::CENTER_CENTER,
                        &p.text,
                        FontId::proportional(20.0),
                        theme::TEAL,
                    );
                    if let Some(opts) = options.filter(|o| o.len() > 1) {
                        let mut sorted: Vec<(&String, &f32)> = opts.iter().collect();
                        sorted.sort_by(|a, b| a.1.total_cmp(b.1));
                        let n = sorted.len().min(6) as f32;
                        let w = (rect.width() - 12.0) / n;
                        for (k, (text, value)) in sorted.iter().take(6).enumerate() {
                            let chip = Rect::from_min_size(
                                Pos2::new(rect.left() + 6.0 + w * k as f32, rect.bottom() - 42.0),
                                egui::vec2(w - 2.0, 18.0),
                            );
                            let current = (**value - p.value).abs() < 1e-4;
                            let fill = if current {
                                theme::MAUVE.gamma_multiply(0.5)
                            } else {
                                theme::SURFACE1
                            };
                            painter.rect_filled(chip, 3.0, fill);
                            painter.with_clip_rect(chip).text(
                                chip.center(),
                                Align2::CENTER_CENTER,
                                text.as_str(),
                                FontId::proportional(10.0),
                                theme::TEXT,
                            );
                            let hovered = ui.rect_contains_pointer(chip);
                            if hovered && ui.input(|i| i.pointer.primary_clicked()) {
                                clicked = Some(**value);
                            }
                        }
                    }
                } else {
                    knob_arc(
                        &painter,
                        Pos2::new(rect.center().x, rect.top() + 58.0),
                        26.0,
                        p.value,
                    );
                    let unit = if p.label.is_empty() {
                        p.text.clone()
                    } else {
                        format!("{} {}", p.text, p.label)
                    };
                    painter.text(
                        Pos2::new(rect.center().x, rect.bottom() - 30.0),
                        Align2::CENTER_CENTER,
                        unit,
                        FontId::proportional(14.0),
                        theme::LAVENDER,
                    );
                }
                painter.text(
                    Pos2::new(rect.center().x, rect.bottom() - 12.0),
                    Align2::CENTER_CENTER,
                    format!("{:.3}", p.value),
                    FontId::monospace(10.0),
                    theme::OVERLAY,
                );
                if read_only {
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
                    }
                } else if let Some(v) = clicked {
                    app.set(id, p.i, v);
                } else if resp.dragged() {
                    let dy = resp.drag_delta().y;
                    if dy != 0.0 {
                        let v = (p.value - dy / DRAG_PX).clamp(0.0, 1.0);
                        app.set(id, p.i, v);
                    }
                }
                resp.on_hover_text(format!("parameter {}: {} = {:.4}", p.i, p.text, p.value));
            }
        });
    });
}

/// One short row per parameter: the name, a bar (or the option chips), the value.
fn show_compact(app: &mut App, ui: &mut egui::Ui, inst: &Instance, read_only: bool) {
    let id = inst.plugin.id;
    egui::ScrollArea::vertical().show(ui, |ui| {
        let width = ui.available_width().min(COMPACT_MAX_W);
        for p in &inst.plugin.params {
            if p.name.ends_with("__open") {
                continue;
            }
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, ROW_H), Sense::hover());
            let painter = ui.painter_at(rect);
            painter.line_segment(
                [rect.left_bottom(), rect.right_bottom()],
                Stroke::new(1.0_f32, theme::SURFACE1),
            );
            let name = Rect::from_min_max(rect.min, Pos2::new(rect.left() + NAME_W, rect.bottom()));
            painter.with_clip_rect(name.shrink(2.0)).text(
                Pos2::new(name.left() + 6.0, name.center().y),
                Align2::LEFT_CENTER,
                &p.name,
                FontId::proportional(13.0),
                theme::TEXT,
            );
            let value = if p.label.is_empty() {
                p.text.clone()
            } else {
                format!("{} {}", p.text, p.label)
            };
            painter.with_clip_rect(rect).text(
                Pos2::new(rect.right() - 6.0, rect.center().y),
                Align2::RIGHT_CENTER,
                value,
                FontId::proportional(13.0),
                theme::TEAL,
            );
            let mid = Rect::from_min_max(
                Pos2::new(rect.left() + NAME_W, rect.top() + 4.0),
                Pos2::new(rect.right() - VALUE_W, rect.bottom() - 4.0),
            );
            let option_like = !p.text.is_empty() && !is_numeric(&p.text);
            let options = inst
                .options
                .get(&p.i)
                .filter(|o| option_like && o.len() > 1);
            let mut set: Option<f32> = None;
            if let Some(opts) = options {
                let mut sorted: Vec<(&String, &f32)> = opts.iter().collect();
                sorted.sort_by(|a, b| a.1.total_cmp(b.1));
                let n = sorted.len().min(8) as f32;
                let w = mid.width() / n;
                for (k, (text, v)) in sorted.iter().take(8).enumerate() {
                    let chip = Rect::from_min_size(
                        Pos2::new(mid.left() + w * k as f32, mid.top()),
                        egui::vec2(w - 3.0, mid.height()),
                    );
                    let current = (**v - p.value).abs() < 1e-4;
                    let fill = if current {
                        theme::MAUVE.gamma_multiply(0.5)
                    } else {
                        theme::SURFACE1
                    };
                    painter.rect_filled(chip, 3.0, fill);
                    painter.with_clip_rect(chip).text(
                        chip.center(),
                        Align2::CENTER_CENTER,
                        text.as_str(),
                        FontId::proportional(11.0),
                        theme::TEXT,
                    );
                    if !read_only
                        && ui
                            .interact(chip, ui.id().with((id, p.i, k)), Sense::click())
                            .clicked()
                    {
                        set = Some(**v);
                    }
                }
            } else {
                painter.rect(
                    mid,
                    3.0,
                    theme::SURFACE0,
                    Stroke::new(1.0_f32, theme::SURFACE1),
                    StrokeKind::Inside,
                );
                let v = p.value.clamp(0.0, 1.0);
                if v > 0.0 {
                    let fill =
                        Rect::from_min_size(mid.min, egui::vec2(mid.width() * v, mid.height()));
                    painter.rect_filled(fill, 3.0, theme::MAUVE);
                }
                let resp = ui.interact(mid, ui.id().with((id, p.i)), Sense::click_and_drag());
                if read_only {
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
                    }
                } else {
                    if resp.hovered() || resp.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    if resp.clicked() || resp.dragged() {
                        if let Some(pos) = resp.interact_pointer_pos() {
                            set =
                                Some(((pos.x - mid.left()) / mid.width().max(1.0)).clamp(0.0, 1.0));
                        }
                    }
                }
                resp.on_hover_text(format!("parameter {}: {} = {:.4}", p.i, p.text, p.value));
            }
            if let Some(v) = set {
                app.set(id, p.i, v);
            }
        }
    });
}

/// A 270-degree knob arc with the value's share lit.
fn knob_arc(painter: &egui::Painter, centre: Pos2, radius: f32, value: f32) {
    let start = 135f32.to_radians();
    let sweep = 270f32.to_radians();
    let arc = |from: f32, to: f32, colour: Color32, width: f32| {
        let n = 32;
        let pts: Vec<Pos2> = (0..=n)
            .map(|i| {
                let a = from + (to - from) * i as f32 / n as f32;
                centre + egui::vec2(a.cos(), a.sin()) * radius
            })
            .collect();
        painter.add(egui::Shape::line(pts, Stroke::new(width, colour)));
    };
    arc(start, start + sweep, theme::SURFACE2, 4.0);
    let v = value.clamp(0.0, 1.0);
    if v > 0.0 {
        arc(start, start + sweep * v, theme::MAUVE, 4.0);
    }
    let a = start + sweep * v;
    let tip = centre + egui::vec2(a.cos(), a.sin()) * (radius - 6.0);
    painter.line_segment([centre, tip], Stroke::new(2.0_f32, theme::ROSEWATER));
    painter.circle_filled(centre, 3.0, theme::ROSEWATER);
}
