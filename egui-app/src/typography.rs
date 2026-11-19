//! The single typography owner for egui: system font selection, fallback,
//! font-size roles and commonly reused text appearances.
//!
//! No font binaries are bundled or redistributed. Keep font-family changes
//! here; keep spacing, colors, pane geometry and control surfaces in theme.rs.

use eframe::egui::{
    self, FontData, FontDefinitions, FontFamily, FontId, RichText, TextStyle,
};
use crate::theme::{SECONDARY, TEXT};

// egui-specific approved roles. The BANDO HUB wordmark was enlarged to 24pt;
// other sizes preserve the React-derived type roles.
pub const FONT_BRAND: f32 = 24.0;
pub const FONT_PAGE_HEADING: f32 = 20.0;
pub const FONT_SECTION_HEADING: f32 = 16.0;
pub const FONT_SUBSECTION_HEADING: f32 = 14.0;
pub const FONT_BODY: f32 = 14.0;
pub const FONT_FIELD_LABEL: f32 = 13.0;
pub const FONT_METADATA: f32 = 12.0;
pub const FONT_MAIN_NAV: f32 = 16.0;
pub const FONT_PRIMARY_ACTION: f32 = 14.0;
pub const FONT_STANDARD_ACTION: f32 = 13.0;

/// Called once during egui initialization, immediately after theme::install.
/// The native UI font is proportional; monospace remains fixed-width with
/// Korean glyphs only as a fallback. Do not change DPI or pane geometry.
pub fn install(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(
        TextStyle::Heading, FontId::proportional(FONT_PAGE_HEADING),
    );
    style.text_styles.insert(
        TextStyle::Body, FontId::proportional(FONT_BODY),
    );
    style.text_styles.insert(
        TextStyle::Button, FontId::proportional(FONT_STANDARD_ACTION),
    );
    style.text_styles.insert(
        TextStyle::Small, FontId::proportional(FONT_METADATA),
    );
    ctx.set_style(style);

    // Use installed fonts only, never a committed font binary. Preserve
    // the original candidate priority and fallback behavior.
    let candidates = [
        r"C:\Windows\Fonts\malgun.ttf",
        r"C:\Windows\Fonts\malgunbd.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    ];
    if let Some((path, bytes)) = candidates.iter()
        .find_map(|path| std::fs::read(path).ok().map(|bytes| (*path, bytes)))
    {
        let mut fonts = FontDefinitions::default();
        fonts.font_data.insert(
            "bando-system-korean".into(),
            FontData::from_owned(bytes).into(),
        );
        fonts.families.entry(FontFamily::Proportional).or_default()
            .insert(0, "bando-system-korean".into());
        fonts.families.entry(FontFamily::Monospace).or_default()
            .push("bando-system-korean".into());
        ctx.set_fonts(fonts);
        eprintln!("[BANDO HUB] Korean UI font: {path}");
    } else {
        eprintln!("[BANDO HUB] Korean UI font was not found; using egui defaults");
    }
}

pub fn brand_heading(text: &str) -> RichText {
    RichText::new(text).size(FONT_BRAND).strong().color(TEXT)
}

pub fn page_heading(text: &str) -> RichText {
    RichText::new(text).size(FONT_PAGE_HEADING).strong().color(TEXT)
}

pub fn field_label(text: &str) -> RichText {
    RichText::new(text).size(FONT_FIELD_LABEL).strong().color(SECONDARY)
}

pub fn section_heading(text: &str) -> RichText {
    RichText::new(text).size(FONT_SECTION_HEADING).strong().color(TEXT)
}

pub fn subsection_heading(text: &str) -> RichText {
    RichText::new(text).size(FONT_SUBSECTION_HEADING).strong().color(TEXT)
}

pub fn body(text: &str) -> RichText {
    RichText::new(text).size(FONT_BODY).color(TEXT)
}

pub fn caption(text: &str) -> RichText {
    RichText::new(text).size(FONT_METADATA).color(SECONDARY)
}
