//! The bundled fonts: Titillium Web (Regular, SemiBold, Bold) and Roboto (Bold only), the two
//! families skins may name, registered under their own family names and as the UI font.

use commander_skin::FontSpec;
use egui::{FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

pub const TITILLIUM_REGULAR: &str = "Titillium Web Regular";
pub const TITILLIUM_SEMIBOLD: &str = "Titillium Web SemiBold";
pub const TITILLIUM_BOLD: &str = "Titillium Web Bold";
pub const ROBOTO_BOLD: &str = "Roboto Bold";

const FONTS: &[(&str, &[u8])] = &[
    (
        TITILLIUM_REGULAR,
        include_bytes!("../../assets/fonts/TitilliumWeb-Regular.ttf"),
    ),
    (
        TITILLIUM_SEMIBOLD,
        include_bytes!("../../assets/fonts/TitilliumWeb-SemiBold.ttf"),
    ),
    (
        TITILLIUM_BOLD,
        include_bytes!("../../assets/fonts/TitilliumWeb-Bold.ttf"),
    ),
    (
        ROBOTO_BOLD,
        include_bytes!("../../assets/fonts/Roboto-Bold.ttf"),
    ),
];

/// Registers the bundled fonts; Titillium Web Regular leads the proportional family, egui's
/// own fonts stay behind it for the glyphs it lacks.
pub fn install(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    for (name, bytes) in FONTS {
        defs.font_data
            .insert((*name).to_string(), Arc::new(FontData::from_static(bytes)));
        defs.families.insert(
            FontFamily::Name((*name).into()),
            vec![(*name).to_string(), TITILLIUM_REGULAR.to_string()],
        );
    }
    if let Some(prop) = defs.families.get_mut(&FontFamily::Proportional) {
        prop.insert(0, TITILLIUM_REGULAR.to_string());
    }
    ctx.set_fonts(defs);
}

/// The family a skin's font request resolves to. Only Roboto Bold is bundled, so every Roboto
/// style draws with it; unknown families draw with the UI font.
pub fn family(spec: &FontSpec) -> FontFamily {
    let style = spec.style.to_ascii_lowercase();
    let name = spec.name.to_ascii_lowercase();
    if name.contains("titillium") {
        let n = if style.contains("semi") {
            TITILLIUM_SEMIBOLD
        } else if style.contains("bold") {
            TITILLIUM_BOLD
        } else {
            TITILLIUM_REGULAR
        };
        FontFamily::Name(n.into())
    } else if name.contains("roboto") {
        FontFamily::Name(ROBOTO_BOLD.into())
    } else {
        FontFamily::Proportional
    }
}

/// The egui size (the em) for a skin's font height. JUCE, and so MPC, sizes a font by its ascent
/// plus descent: 1.521 em for Titillium Web and 1.172 em for Roboto (their `hhea` metrics).
pub fn size(spec: &FontSpec) -> f32 {
    let line = match family(spec) {
        FontFamily::Name(n) if &*n == ROBOTO_BOLD => 1.172,
        _ => 1.521,
    };
    spec.height / line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str, style: &str) -> FontSpec {
        FontSpec {
            name: name.into(),
            style: style.into(),
            height: 20.0,
        }
    }

    #[test]
    fn families_resolve() {
        assert_eq!(
            family(&spec("Titillium Web", "SemiBold")),
            FontFamily::Name(TITILLIUM_SEMIBOLD.into())
        );
        assert_eq!(
            family(&spec("Titillium Web", "Bold")),
            FontFamily::Name(TITILLIUM_BOLD.into())
        );
        assert_eq!(
            family(&spec("Titillium Web", "Light")),
            FontFamily::Name(TITILLIUM_REGULAR.into())
        );
        assert_eq!(
            family(&spec("Roboto", "Regular")),
            FontFamily::Name(ROBOTO_BOLD.into())
        );
        assert_eq!(family(&spec("Comic", "Regular")), FontFamily::Proportional);
    }

    #[test]
    fn height_is_ascent_plus_descent() {
        let mut t = spec("Titillium Web", "Regular");
        t.height = 55.0;
        assert!((size(&t) - 36.16).abs() < 0.01);
        let mut r = spec("Roboto", "Bold");
        r.height = 23.44;
        assert!((size(&r) - 20.0).abs() < 0.01);
    }

    #[test]
    fn fonts_load_and_cover_ascii() {
        let ctx = egui::Context::default();
        install(&ctx);
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
        ctx.fonts_mut(|f| {
            for name in [
                TITILLIUM_REGULAR,
                TITILLIUM_SEMIBOLD,
                TITILLIUM_BOLD,
                ROBOTO_BOLD,
            ] {
                let id = egui::FontId::new(16.0, FontFamily::Name(name.into()));
                assert!(f.has_glyph(&id, 'A'), "{name}");
                assert!(f.has_glyph(&id, '#'), "{name}");
            }
        });
    }
}
