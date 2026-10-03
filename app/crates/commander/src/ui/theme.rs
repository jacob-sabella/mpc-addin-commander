//! A dark palette (Catppuccin Mocha tones) applied to egui's visuals.

use egui::{Color32, Stroke, Visuals};

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub const ROSEWATER: Color32 = rgb(0xf5, 0xe0, 0xdc);
pub const MAUVE: Color32 = rgb(0xcb, 0xa6, 0xf7);
pub const RED: Color32 = rgb(0xf3, 0x8b, 0xa8);
pub const PEACH: Color32 = rgb(0xfa, 0xb3, 0x87);
pub const YELLOW: Color32 = rgb(0xf9, 0xe2, 0xaf);
pub const GREEN: Color32 = rgb(0xa6, 0xe3, 0xa1);
pub const TEAL: Color32 = rgb(0x94, 0xe2, 0xd5);
pub const SAPPHIRE: Color32 = rgb(0x74, 0xc7, 0xec);
pub const BLUE: Color32 = rgb(0x89, 0xb4, 0xfa);
pub const LAVENDER: Color32 = rgb(0xb4, 0xbe, 0xfe);
pub const TEXT: Color32 = rgb(0xcd, 0xd6, 0xf4);
pub const SUBTEXT: Color32 = rgb(0xa6, 0xad, 0xc8);
pub const OVERLAY: Color32 = rgb(0x6c, 0x70, 0x86);
pub const SURFACE2: Color32 = rgb(0x58, 0x5b, 0x70);
pub const SURFACE1: Color32 = rgb(0x45, 0x47, 0x5a);
pub const SURFACE0: Color32 = rgb(0x31, 0x32, 0x44);
pub const BASE: Color32 = rgb(0x1e, 0x1e, 0x2e);
pub const MANTLE: Color32 = rgb(0x18, 0x18, 0x25);
pub const CRUST: Color32 = rgb(0x11, 0x11, 0x1b);

/// The window clear colour: `BASE`, written straight to the non-sRGB framebuffer.
pub const CLEAR: wgpu::Color = wgpu::Color {
    r: 0x1e as f64 / 255.0,
    g: 0x1e as f64 / 255.0,
    b: 0x2e as f64 / 255.0,
    a: 1.0,
};

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.override_text_color = Some(TEXT);
    v.panel_fill = BASE;
    v.window_fill = MANTLE;
    v.extreme_bg_color = CRUST;
    v.faint_bg_color = SURFACE0;
    v.code_bg_color = CRUST;
    v.hyperlink_color = SAPPHIRE;
    v.warn_fg_color = PEACH;
    v.error_fg_color = RED;
    v.selection.bg_fill = MAUVE.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0_f32, MAUVE);
    v.widgets.noninteractive.bg_fill = SURFACE0;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, SURFACE1);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, SUBTEXT);
    v.widgets.inactive.bg_fill = SURFACE0;
    v.widgets.inactive.weak_bg_fill = SURFACE0;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.hovered.bg_fill = SURFACE1;
    v.widgets.hovered.weak_bg_fill = SURFACE1;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, OVERLAY);
    v.widgets.hovered.fg_stroke = Stroke::new(1.5_f32, ROSEWATER);
    v.widgets.active.bg_fill = SURFACE2;
    v.widgets.active.weak_bg_fill = SURFACE2;
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, MAUVE);
    v.widgets.active.fg_stroke = Stroke::new(2.0_f32, ROSEWATER);
    v.widgets.open.bg_fill = SURFACE1;
    v.widgets.open.weak_bg_fill = SURFACE1;
    v.window_stroke = Stroke::new(1.0_f32, SURFACE1);
    v.window_corner_radius = 6.0.into();
    v.menu_corner_radius = 6.0.into();
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
    });
}
