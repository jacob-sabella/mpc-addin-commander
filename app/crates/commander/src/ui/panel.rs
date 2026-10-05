//! Draws a page of a plugin's skin from its draw list, and turns clicks and drags on it into
//! `set` messages.

use super::{colour, fonts, theme, App, Drag};
use crate::model::{Instance, SkinBundle};
use commander_skin::{hit, Action, HAlign, Item, LabelStyle, VAlign};
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind};

const UV_FULL: Rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0));

pub fn show(
    app: &mut App,
    ctx: &egui::Context,
    ui: &mut egui::Ui,
    inst: &Instance,
    bundle: &SkinBundle,
    page: usize,
) {
    let Some(page_info) = bundle.skin.pages().get(page) else {
        return;
    };
    let avail = ui.available_size();
    let scale = (avail.x / page_info.size.w)
        .min(avail.y / page_info.size.h)
        .max(0.05);
    let size = egui::vec2(page_info.size.w * scale, page_info.size.h * scale);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let origin = rect.min;
    let to_screen = |r: commander_skin::Rect| {
        Rect::from_min_size(
            origin + egui::vec2(r.x, r.y) * scale,
            egui::vec2(r.w, r.h) * scale,
        )
    };
    let layout = bundle.skin.layout(page, inst);
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::BLACK);
    let uid = inst.plugin.uid.clone();
    for item in &layout.items {
        match item {
            Item::Image { rect, file } | Item::Button { rect, file, .. } => {
                let r = to_screen(*rect);
                match app.texture(ctx, &uid, file, None, bundle) {
                    Some(tex) => {
                        painter.image(tex, r, UV_FULL, Color32::WHITE);
                    }
                    None => placeholder(&painter, r),
                }
            }
            Item::Knob {
                rect,
                file,
                frames,
                frame,
                ..
            } => {
                let r = to_screen(*rect);
                match app.texture(ctx, &uid, file, Some((*frame, *frames)), bundle) {
                    Some(tex) => {
                        painter.image(tex, r, UV_FULL, Color32::WHITE);
                    }
                    None => placeholder(&painter, r),
                }
            }
            Item::Slider {
                rect,
                thumb,
                vertical,
                value,
                ..
            } => {
                let r = to_screen(*rect);
                let Some(img) = bundle.images.get(thumb) else {
                    placeholder(&painter, r);
                    continue;
                };
                let size = egui::vec2(img.width as f32, img.height as f32) * scale;
                let v = value.clamp(0.0, 1.0);
                let min = if *vertical {
                    Pos2::new(
                        r.center().x - size.x / 2.0,
                        r.bottom() - size.y - (r.height() - size.y).max(0.0) * v,
                    )
                } else {
                    Pos2::new(
                        r.left() + (r.width() - size.x).max(0.0) * v,
                        r.center().y - size.y / 2.0,
                    )
                };
                if let Some(tex) = app.texture(ctx, &uid, thumb, None, bundle) {
                    painter.image(tex, Rect::from_min_size(min, size), UV_FULL, Color32::WHITE);
                }
            }
            Item::Arrow { rect, colour: c } => {
                let r = to_screen(*rect);
                painter.add(egui::Shape::convex_polygon(
                    vec![r.left_top(), r.right_top(), r.center_bottom()],
                    colour(*c),
                    Stroke::NONE,
                ));
            }
            Item::Label {
                rect, text, style, ..
            } => draw_label(&painter, to_screen(*rect), text, style, scale),
            Item::Meter { rect, value, .. } => {
                let r = to_screen(*rect);
                painter.rect_filled(r, 2.0, theme::SURFACE0);
                let fill = Rect::from_min_max(
                    Pos2::new(r.left(), r.bottom() - r.height() * value.clamp(0.0, 1.0)),
                    r.max,
                );
                painter.rect_filled(fill, 2.0, theme::GREEN);
            }
            Item::Generic {
                rect, name, param, ..
            } => {
                let r = to_screen(*rect);
                painter.rect(
                    r,
                    3.0,
                    theme::SURFACE0,
                    Stroke::new(1.0_f32, theme::OVERLAY),
                    StrokeKind::Inside,
                );
                let text = match param {
                    Some(p) => {
                        let p = inst.param(*p);
                        format!(
                            "{}\n{}",
                            p.map_or(name.as_str(), |p| p.name.as_str()),
                            p.map_or("", |p| p.text.as_str())
                        )
                    }
                    None => name.clone(),
                };
                painter.text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    text,
                    FontId::proportional(14.0 * scale.max(0.5)),
                    theme::TEXT,
                );
            }
        }
    }

    // Input: a press starts a set or a drag on the topmost control under the pointer.
    let pressed = ui.input(|i| i.pointer.primary_pressed());
    let down = ui.input(|i| i.pointer.primary_down());
    let pos = ui.input(|i| i.pointer.interact_pos());
    let id = inst.plugin.id;
    if pressed && app.drag.is_none() && resp.contains_pointer() {
        if let Some(p) = pos.filter(|p| rect.contains(*p)) {
            let (x, y) = ((p.x - origin.x) / scale, (p.y - origin.y) / scale);
            if let Some(c) = hit(&layout.controls, x, y) {
                match c.press(inst) {
                    Some(Action::Set { param, value }) => app.set(id, param, value),
                    Some(Action::Drag {
                        param,
                        height,
                        start,
                    }) => {
                        app.drag = Some(Drag {
                            id,
                            param,
                            start_value: start,
                            start_y: p.y,
                            px_per_unit: (height * scale).max(1.0),
                            last_sent: start,
                        });
                    }
                    None => {}
                }
            }
        }
    }
    let mut send: Option<(u32, u32, f32)> = None;
    match &mut app.drag {
        Some(d) if d.id == id => {
            if down {
                if let Some(p) = pos {
                    let v = (d.start_value + (d.start_y - p.y) / d.px_per_unit).clamp(0.0, 1.0);
                    if (v - d.last_sent).abs() > 0.0005 {
                        d.last_sent = v;
                        send = Some((id, d.param, v));
                    }
                }
            } else {
                app.drag = None;
            }
        }
        Some(_) if !down => app.drag = None,
        _ => {}
    }
    if let Some((id, param, v)) = send {
        app.set(id, param, v);
    }
    if let Some(p) = pos.filter(|p| rect.contains(*p)) {
        let (x, y) = ((p.x - origin.x) / scale, (p.y - origin.y) / scale);
        if let Some(c) = hit(&layout.controls, x, y) {
            if let Some(param) = c.param {
                if let Some(pm) = inst.param(param) {
                    resp.clone()
                        .on_hover_text(format!("{} = {} ({:.3})", pm.name, pm.text, pm.value));
                }
            }
        }
    }
}

fn placeholder(painter: &egui::Painter, r: Rect) {
    painter.rect_stroke(
        r,
        0.0,
        Stroke::new(1.0_f32, theme::SURFACE1),
        StrokeKind::Inside,
    );
}

/// A label as JUCE would place it: the text anchored by its justification inside the box.
fn draw_label(painter: &egui::Painter, r: Rect, text: &str, style: &LabelStyle, scale: f32) {
    if text.is_empty() {
        return;
    }
    let font = FontId::new(
        (style.font.height * scale).max(1.0),
        fonts::family(&style.font),
    );
    let (x, ax) = match style.justification.h {
        HAlign::Left => (r.left(), egui::Align::LEFT),
        HAlign::Centre => (r.center().x, egui::Align::Center),
        HAlign::Right => (r.right(), egui::Align::RIGHT),
    };
    let (y, ay) = match style.justification.v {
        VAlign::Top => (r.top(), egui::Align::TOP),
        VAlign::Centre => (r.center().y, egui::Align::Center),
        VAlign::Bottom => (r.bottom(), egui::Align::BOTTOM),
    };
    let clip = r.expand2(egui::vec2(0.0, 3.0 * scale));
    painter.with_clip_rect(clip).text(
        Pos2::new(x, y),
        Align2([ax, ay]),
        text,
        font,
        colour(style.colour),
    );
}
