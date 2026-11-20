//! BANDO HUB typography design system.
//! This module owns typeface, real regular/bold faces, role-based size,
//! tracking, line-height, contrast and numeric alignment. Panels, spacing,
//! colors and business decisions keep their existing owners.
//!
//! The wordmark has an explicit optical size and alignment. No font
//! binaries are bundled or redistributed.

use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, RichText, TextStyle,
};
use crate::theme::{MUTED, SECONDARY, TEXT};

// Caps height is visually close to the 38pt brand icon (whose bitmap
// includes transparent margins). Do not alter overall header geometry.
pub const FONT_BRAND: f32 = 28.0;
/// The font galley includes ascent/descent whitespace, so centering its box
/// places the visible capital letters above the icon and navigation center.
/// Compensate in this one typography token without moving the header row.
pub const BRAND_OPTICAL_CENTER_Y: f32 = 5.0;
pub const FONT_PAGE_HEADING: f32 = 20.0;
pub const FONT_MODAL_TITLE: f32 = 18.0;
pub const FONT_SECTION_HEADING: f32 = 16.0;
pub const FONT_SUBSECTION_HEADING: f32 = 14.0;
pub const FONT_BODY: f32 = 14.0;
pub const FONT_FIELD_LABEL: f32 = 13.0;
pub const FONT_METADATA: f32 = 12.0;
pub const FONT_MAIN_NAV: f32 = 16.0;
pub const FONT_PRIMARY_ACTION: f32 = 14.0;
pub const FONT_STANDARD_ACTION: f32 = 13.0;
// Shared line boxes used for balanced reception guidance geometry.
pub const SUBSECTION_LINE_HEIGHT: f32 = 21.0;
pub const METADATA_LINE_HEIGHT: f32 = 18.0;

const EMPHASIS_FAMILY: &str = "bando-ui-emphasis";
const BRAND_FAMILY: &str = "bando-brand-display";

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
    ModalTitle,
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
            size: FONT_BRAND, line: 34.0, tracking: -0.35,
            color: TEXT, weight: Weight::Emphasis,
        },
        Role::Page => TypeSpec {
            size: FONT_PAGE_HEADING, line: 28.0, tracking: -0.30,
            color: TEXT, weight: Weight::Emphasis,
        },
        Role::ModalTitle => TypeSpec {
            size: FONT_MODAL_TITLE, line: 24.0, tracking: -0.10,
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
            size: FONT_SUBSECTION_HEADING, line: SUBSECTION_LINE_HEIGHT, tracking: -0.10,
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
            size: FONT_METADATA, line: METADATA_LINE_HEIGHT, tracking: 0.0,
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
            size: FONT_SECTION_HEADING, line: 23.0, tracking: 0.0,
            color: TEXT, weight: Weight::Monospace,
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
    let is_brand = matches!(role, Role::Brand);
    let role = spec(role);
    let rich = RichText::new(text)
        .size(role.size)
        .line_height(Some(role.line))
        .extra_letter_spacing(role.tracking)
        .color(role.color);
    match role.weight {
        Weight::Regular => rich,
        Weight::Emphasis => {
            let family = if is_brand { BRAND_FAMILY } else { EMPHASIS_FAMILY };
            rich.family(FontFamily::Name(family.into())).strong()
        },
        Weight::Monospace => rich.family(FontFamily::Monospace),
    }
}

pub fn brand_heading(text: &str) -> RichText { render(text, Role::Brand) }

/// Layout the wordmark using the same font, tracking and line-height as the
/// typography role. The header paints this galley in an explicit centered
/// rect so nested egui row layouts cannot bias its vertical position.
pub fn brand_galley(ui: &egui::Ui, text: &str) -> std::sync::Arc<egui::Galley> {
    let role = spec(Role::Brand);
    let format = egui::TextFormat {
        font_id: FontId::new(
            role.size, FontFamily::Name(BRAND_FAMILY.into())
        ),
        color: role.color,
        extra_letter_spacing: role.tracking,
        // Use actual font metrics for measured vertical centering; a forced
        // 42pt line box placed the visible capitals off-center against the icon.
        line_height: None,
        ..Default::default()
    };
    let job = egui::text::LayoutJob::single_section(text.to_owned(), format);
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

pub fn page_heading(text: &str) -> RichText { render(text, Role::Page) }
pub fn modal_title(text: &str) -> RichText { render(text, Role::ModalTitle) }
pub fn section_heading(text: &str) -> RichText { render(text, Role::Section) }

/// Same bold face, tracking and size as the shared section heading, but
/// laid out explicitly for the workflow card title. Its painted x-coordinate
/// is fixed by the caller, never by a centered Label allocation.
pub fn section_heading_galley(
    ui: &egui::Ui, text: &str
) -> std::sync::Arc<egui::Galley> {
    let role = spec(Role::Section);
    let format = egui::TextFormat {
        font_id: FontId::new(
            role.size, FontFamily::Name(EMPHASIS_FAMILY.into())
        ),
        color: role.color,
        extra_letter_spacing: role.tracking,
        line_height: Some(role.line),
        ..Default::default()
    };
    let job = egui::text::LayoutJob::single_section(text.to_owned(), format);
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}
pub fn subsection_heading(text: &str) -> RichText { render(text, Role::Subsection) }
pub fn body(text: &str) -> RichText { render(text, Role::Body) }
pub fn field_label(text: &str) -> RichText { render(text, Role::FieldLabel) }

/// Shared field-label typography laid out for exact left-edge painting.
/// A fixed-width egui Label can otherwise center its glyphs inside the
/// recipient label column, making short labels drift toward the input.
pub fn field_label_galley(
    ui: &egui::Ui, text: &str
) -> std::sync::Arc<egui::Galley> {
    let role = spec(Role::FieldLabel);
    let format = egui::TextFormat {
        font_id: FontId::new(
            role.size, FontFamily::Name(EMPHASIS_FAMILY.into())
        ),
        color: role.color,
        extra_letter_spacing: role.tracking,
        line_height: Some(role.line),
        ..Default::default()
    };
    let job = egui::text::LayoutJob::single_section(text.to_owned(), format);
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}
pub fn caption(text: &str) -> RichText { render(text, Role::Metadata) }
pub fn table_header(text: &str) -> RichText { render(text, Role::TableHeader) }

/// Measured header glyphs for the fixed three-column receipt table.
/// Unlike Label::halign in Ui::put, this galley is painted at each column's
/// literal left/right anchor, using the existing bold field-label font.
pub fn table_header_galley(
    ui: &egui::Ui, text: &str
) -> std::sync::Arc<egui::Galley> {
    let role = spec(Role::TableHeader);
    let format = egui::TextFormat {
        font_id: FontId::new(
            role.size, FontFamily::Name(EMPHASIS_FAMILY.into())
        ),
        color: role.color,
        extra_letter_spacing: role.tracking,
        line_height: Some(role.line),
        ..Default::default()
    };
    let job = egui::text::LayoutJob::single_section(text.to_owned(), format);
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}
/// Measured text for tightly packed client directory rows. Unlike egui
/// labels inside fixed subcells, a galley can be painted at the exact same
/// left coordinate as the column header without implicit centering.
fn client_table_galley(
    ui: &egui::Ui, text: &str, role: Role, emphasis: bool, color: Color32,
) -> std::sync::Arc<egui::Galley> {
    let style=spec(role);
    let font_family=if emphasis {
        FontFamily::Name(EMPHASIS_FAMILY.into())
    } else {
        FontFamily::Proportional
    };
    let format=egui::TextFormat {
        font_id: FontId::new(style.size,font_family),
        color,
        extra_letter_spacing: style.tracking,
        line_height: Some(style.line),
        ..Default::default()
    };
    ui.fonts_mut(|fonts|fonts.layout_job(
        egui::text::LayoutJob::single_section(text.to_owned(),format)
    ))
}
pub fn client_table_cell_galley(
    ui: &egui::Ui, text: &str, emphasized: bool,
) -> std::sync::Arc<egui::Galley> {
    client_table_galley(ui,text,Role::TableCell,emphasized,TEXT)
}
pub fn client_table_metadata_galley(
    ui: &egui::Ui, text: &str, color: Color32,
) -> std::sync::Arc<egui::Galley> {
    client_table_galley(ui,text,Role::Metadata,false,color)
}

pub fn table_cell(text: &str) -> RichText { render(text, Role::TableCell) }
pub fn summary_value(text: &str) -> RichText { render(text, Role::SummaryValue) }

/// Freight overview cards reuse the standard 14pt body role for labels
/// and the 13pt table-cell role for units. All glyphs use the summary
/// value's 23pt line box, so 14/16/13pt text shares one vertical center.
fn summary_card_secondary_galley(
    ui: &egui::Ui, text: &str, role: Role
) -> std::sync::Arc<egui::Galley> {
    let style = spec(role);
    let format = egui::TextFormat {
        font_id: FontId::new(style.size, FontFamily::Proportional),
        color: SECONDARY,
        extra_letter_spacing: style.tracking,
        line_height: Some(spec(Role::SummaryValue).line),
        ..Default::default()
    };
    ui.fonts_mut(|fonts| fonts.layout_job(
        egui::text::LayoutJob::single_section(text.to_owned(), format)
    ))
}
pub fn summary_card_label_galley(
    ui: &egui::Ui, text: &str
) -> std::sync::Arc<egui::Galley> {
    summary_card_secondary_galley(ui, text, Role::Body)
}
pub fn summary_card_unit_galley(
    ui: &egui::Ui, text: &str
) -> std::sync::Arc<egui::Galley> {
    summary_card_secondary_galley(ui, text, Role::TableCell)
}

/// Produce the same summary font as summary_value(), but as measured glyphs
/// for a fixed-position card row without nested automatic label alignment.
pub fn summary_value_galley(
    ui: &egui::Ui, text: &str
) -> std::sync::Arc<egui::Galley> {
    let role = spec(Role::SummaryValue);
    let format = egui::TextFormat {
        font_id: FontId::new(role.size, FontFamily::Monospace),
        color: role.color,
        extra_letter_spacing: role.tracking,
        line_height: Some(role.line),
        ..Default::default()
    };
    ui.fonts_mut(|fonts| fonts.layout_job(
        egui::text::LayoutJob::single_section(text.to_owned(), format)
    ))
}
pub fn quantity(text: &str) -> RichText { render(text, Role::Quantity) }

/// Explicit numeric input typography: use Consolas (or system monospace
/// fallback) for editable money and quantity fields as well as display.
pub fn numeric_editor_font() -> FontId {
    FontId::monospace(FONT_BODY)
}

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
        FontFamily::Name(EMPHASIS_FAMILY.into()), emphasis_fonts.clone(),
    );

    // Brand is an English-only wordmark: use a distinct, industrial Latin
    // display face, not the same face as every Korean UI label. Missing
    // Windows fonts fall back to the registered Korean emphasis family.
    let brand_candidates = [
        r"C:\Windows\Fonts\bahnschrift.ttf",
        r"C:\Windows\Fonts\seguisb.ttf",
    ];
    let brand = brand_candidates.iter()
        .find_map(|path| std::fs::read(path).ok().map(|bytes| (*path, bytes)));
    let mut brand_fonts = Vec::new();
    if let Some((_, bytes)) = &brand {
        let key = "bando-brand-latin";
        fonts.font_data.insert(key.into(), FontData::from_owned(bytes.clone()).into());
        brand_fonts.push(key.into());
    }
    brand_fonts.extend(emphasis_fonts);
    fonts.families.insert(FontFamily::Name(BRAND_FAMILY.into()), brand_fonts);

    // Keep Windows Consolas ahead of the egui monospace fallback for
    // quantities and numeric totals. Korean glyphs still fall back to
    // the regular UI face in the existing monospace family.
    let numeric_candidates = [
        r"C:\Windows\Fonts\consola.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    ];
    let numeric = numeric_candidates.iter()
        .find_map(|path| std::fs::read(path).ok().map(|bytes| (*path, bytes)));
    if let Some((_, bytes)) = &numeric {
        let key = "bando-numeric-mono";
        fonts.font_data.insert(key.into(), FontData::from_owned(bytes.clone()).into());
        fonts.families.entry(FontFamily::Monospace).or_default().insert(0, key.into());
    }
    ctx.set_fonts(fonts);

    match regular_name {
        Some(path) => eprintln!("[BANDO HUB] UI typeface: {path}"),
        None => eprintln!("[BANDO HUB] UI typeface fallback: egui default"),
    }
    if bold.is_none() {
        eprintln!("[BANDO HUB] Emphasis face: system bold unavailable; fallback in use");
    }
    match brand {
        Some((path, _)) => eprintln!("[BANDO HUB] Brand display face: {path}"),
        None => eprintln!("[BANDO HUB] Brand display face: Korean emphasis fallback"),
    }
    match numeric {
        Some((path, _)) => eprintln!("[BANDO HUB] Numeric face: {path}"),
        None => eprintln!("[BANDO HUB] Numeric face: egui monospace fallback"),
    }
}
