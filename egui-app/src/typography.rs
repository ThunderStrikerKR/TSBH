//! BANDO HUB typography design system.
//! This module owns typeface, real regular/bold faces, role-based size,
//! tracking, line-height, contrast and numeric alignment. Panels, spacing,
//! colors and business decisions keep their existing owners.
//!
//! The approved 24pt wordmark is an intentional egui override. No font
//! binaries are bundled or redistributed.

use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, RichText, TextStyle,
};
use crate::theme::{MUTED, SECONDARY, TEXT};

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

const EMPHASIS_FAMILY: &str = "bando-ui-emphasis";

#[derive(Clone, Copy)]
enum Weight {
    Regular,
    Emphasis,
    Monospace,
}

/// Font-size alone is not a hierarchy: line-height, tracking, contrast and
/// face weight are designed together. The values fit the existing fixed
/// 38pt section header, 36pt input and 34/38pt button heights.
#[derive(Clone, Copy)]
struct TypeSpec {
    size: f32,
    line: f32,
    tracking: f32,
    color: Color32,
    weight: Weight,
}

#[derive(Clone, Copy)]
enum Role {
    Brand,
    Page,
    Navigation,
    NavigationActive,
    Section,
    Subsection,
    Body,
    FieldLabel,
    Metadata,
    TableHeader,
    TableCell,
    SummaryValue,
    Quantity,
    Status,
}

fn spec(role: Role) -> TypeSpec {
    match role {
        Role::Brand => TypeSpec {
            size: FONT_BRAND, line: 30.0, tracking: -0.60,
            color: TEXT, weight: Weight::Emphasis,
        },
        Role::Page => TypeSpec {
            size: FONT_PAGE_HEADING, line: 28.0, tracking: -0.30,
            color: TEXT, weight: Weight::Emphasis,
        },
        Role::Navigation => TypeSpec {
            size: FONT_MAIN_NAV, line: 23.0, tracking: -0.16,
            color: SECONDARY, weight: Weight::Regular,
        },
        Role::NavigationActive => TypeSpec {
            size: FONT_MAIN_NAV, line: 23.0, tracking: -0.16,
            color: TEXT, weight: Weight::Emphasis,
        },
        Role::Section => TypeSpec {
            size: FONT_SECTION_HEADING, line: 23.0, tracking: -0.20,
            color: TEXT, weight: Weight::Emphasis,
        },
        Role::Subsection => TypeSpec {
            size: FONT_SUBSECTION_HEADING, line: 21.0, tracking: -0.10,
            color: TEXT, weight: Weight::Emphasis,
        },
        Role::Body => TypeSpec {
            size: FONT_BODY, line: 22.0, tracking: 0.0,
            color: TEXT, weight: Weight::Regular,
        },
        Role::FieldLabel => TypeSpec {
            size: FONT_FIELD_LABEL, line: 19.0, tracking: 0.0,
            color: SECONDARY, weight: Weight::Emphasis,
        },
        Role::Metadata => TypeSpec {
            size: FONT_METADATA, line: 18.0, tracking: 0.0,
            color: MUTED, weight: Weight::Regular,
        },
        Role::TableHeader => TypeSpec {
            size: FONT_FIELD_LABEL, line: 19.0, tracking: 0.0,
            color: SECONDARY, weight: Weight::Emphasis,
        },
        Role::TableCell => TypeSpec {
            size: FONT_FIELD_LABEL, line: 20.0, tracking: 0.0,
            color: TEXT, weight: Weight::Regular,
        },
        Role::SummaryValue => TypeSpec {
            size: FONT_SECTION_HEADING, line: 23.0, tracking: -0.10,
            color: TEXT, weight: Weight::Emphasis,
        },
        Role::Quantity => TypeSpec {
            size: FONT_FIELD_LABEL, line: 20.0, tracking: 0.0,
            color: SECONDARY, weight: Weight::Monospace,
        },
        Role::Status => TypeSpec {
            size: FONT_FIELD_LABEL, line: 20.0, tracking: 0.0,
            color: TEXT, weight: Weight::Emphasis,
        },
    }
}

fn render(text: &str, role: Role) -> RichText {
    let role = spec(role);
    let rich = RichText::new(text)
        .size(role.size)
        .line_height(Some(role.line))
        .extra_letter_spacing(role.tracking)
        .color(role.color);
    match role.weight {
        Weight::Regular => rich,
        Weight::Emphasis => rich.family(FontFamily::Name(EMPHASIS_FAMILY.into())).strong(),
        Weight::Monospace => rich.family(FontFamily::Monospace),
    }
}

pub fn brand_heading(text: &str) -> RichText { render(text, Role::Brand) }
pub fn page_heading(text: &str) -> RichText { render(text, Role::Page) }
pub fn section_heading(text: &str) -> RichText { render(text, Role::Section) }
pub fn subsection_heading(text: &str) -> RichText { render(text, Role::Subsection) }
pub fn body(text: &str) -> RichText { render(text, Role::Body) }
pub fn field_label(text: &str) -> RichText { render(text, Role::FieldLabel) }
pub fn caption(text: &str) -> RichText { render(text, Role::Metadata) }
pub fn table_header(text: &str) -> RichText { render(text, Role::TableHeader) }
pub fn table_cell(text: &str) -> RichText { render(text, Role::TableCell) }
pub fn summary_value(text: &str) -> RichText { render(text, Role::SummaryValue) }
pub fn quantity(text: &str) -> RichText { render(text, Role::Quantity) }

pub fn status(text: &str, color: Color32) -> RichText {
    render(text, Role::Status).color(color)
}

/// One button-text policy: selected tabs and high-priority actions are bold;
/// low-priority/inactive actions remain regular. Color belongs to the theme.
pub fn action_label(text: &str, size: f32, selected: bool, color: Color32) -> RichText {
    let role = if size >= FONT_MAIN_NAV {
        if selected { Role::NavigationActive } else { Role::Navigation }
    } else {
        Role::TableCell
    };
    let (height, tracking) = if size >= FONT_MAIN_NAV {
        (23.0, -0.16)
    } else if size >= FONT_PRIMARY_ACTION {
        (20.0, -0.10)
    } else {
        (19.0, 0.0)
    };
    let mut rich = render(text, role)
        .size(size)
        .line_height(Some(height))
        .extra_letter_spacing(tracking)
        .color(color);
    if selected && size < FONT_MAIN_NAV {
        rich = rich.family(FontFamily::Name(EMPHASIS_FAMILY.into())).strong();
    }
    rich
}

/// Install real regular/bold installed faces, not only simulated boldness.
/// If the bold file is unavailable, the named emphasis family explicitly
/// falls back to proportional fonts and text retains a strong semantic role.
pub fn install(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.text_styles.insert(TextStyle::Heading, FontId::proportional(FONT_PAGE_HEADING));
    style.text_styles.insert(TextStyle::Body, FontId::proportional(FONT_BODY));
    style.text_styles.insert(TextStyle::Button, FontId::proportional(FONT_STANDARD_ACTION));
    style.text_styles.insert(TextStyle::Small, FontId::proportional(FONT_METADATA));
    ctx.set_style(style);

    let normal_candidates = [
        r"C:\Windows\Fonts\malgun.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    ];
    let bold_candidates = [
        r"C:\Windows\Fonts\malgunbd.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Bold.ttc",
    ];

    let mut fonts = FontDefinitions::default();
    let mut regular_name = None;
    if let Some((path, bytes)) = normal_candidates.iter()
        .find_map(|path| std::fs::read(path).ok().map(|bytes| (*path, bytes)))
    {
        let key = "bando-ui-regular";
        fonts.font_data.insert(key.into(), FontData::from_owned(bytes).into());
        fonts.families.entry(FontFamily::Proportional).or_default()
            .insert(0, key.into());
        fonts.families.entry(FontFamily::Monospace).or_default()
            .push(key.into());
        regular_name = Some(path);
    }

    let bold = bold_candidates.iter()
        .find_map(|path| std::fs::read(path).ok().map(|bytes| (*path, bytes)));
    let mut emphasis_fonts = Vec::new();
    if let Some((_, bytes)) = &bold {
        let key = "bando-ui-bold";
        fonts.font_data.insert(key.into(), FontData::from_owned(bytes.clone()).into());
        emphasis_fonts.push(key.into());
    }
    // Always register this family, including when neither system face exists.
    emphasis_fonts.extend(
        fonts.families.get(&FontFamily::Proportional)
            .cloned().unwrap_or_default()
    );
    fonts.families.insert(
        FontFamily::Name(EMPHASIS_FAMILY.into()), emphasis_fonts,
    );
    ctx.set_fonts(fonts);

    match regular_name {
        Some(path) => eprintln!("[BANDO HUB] UI typeface: {path}"),
        None => eprintln!("[BANDO HUB] UI typeface fallback: egui default"),
    }
    if bold.is_none() {
        eprintln!("[BANDO HUB] Emphasis face: system bold unavailable; fallback in use");
    }
}
