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
// Centered, full-pane native OCR blocking state in Reception.
pub const FONT_OCR_PROGRESS_TITLE: f32 = 28.0;
// Readable side-by-side OCR source verification. All reception source
// documents use this same 16pt size; general body text remains 14pt.
pub const FONT_OCR_REVIEW_TEXT: f32 = 16.0;
pub const FONT_RECEPTION_SOURCE_TEXT: f32 = FONT_OCR_REVIEW_TEXT;
// Recipient detail INPUT values share the 16pt reception source/OCR size.
// Field labels and other 14pt body content retain their existing hierarchy.
pub const FONT_RECEPTION_RECIPIENT_TEXT: f32 = FONT_OCR_REVIEW_TEXT;
// The source-linked item/quantity/amount and freight values share 16pt.
pub const FONT_RECEPTION_ITEM_TEXT: f32 = FONT_OCR_REVIEW_TEXT;
pub const FONT_MODAL_TITLE: f32 = 18.0;
pub const FONT_SECTION_HEADING: f32 = 16.0;
pub const FONT_SUBSECTION_HEADING: f32 = 14.0;
pub const FONT_BODY: f32 = 14.0;
pub const FONT_FIELD_LABEL: f32 = 13.0;
pub const FONT_METADATA: f32 = 12.0;
pub const FONT_MAIN_NAV: f32 = 18.0;
/// Second-level management tabs remain full-size controls, visually
/// subordinate through position and neutral styling rather than tiny text.
pub const FONT_MANAGEMENT_TAB: f32 = 16.0;
/// Visual hierarchy of freight KPI cards: prominent value, supporting caption.
pub const FONT_FREIGHT_SUMMARY_NUMBER: f32 = 28.0;
pub const FONT_FREIGHT_SUMMARY_LABEL: f32 = 13.0;
pub const FONT_PRIMARY_ACTION: f32 = 14.0;
pub const FONT_STANDARD_ACTION: f32 = 13.0;

/// Business data typography is classified by semantic value, never by the
/// screen that happens to contain it. Monetary amounts and quantities use
/// Consolas (monospace) for stable columns. Dates and phone identifiers use
/// the regular proportional Korean UI family, matching normal source text.
/// Sizes follow the approved presentation context, not digit count.
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum DataValueKind { Money, Quantity, Date, Phone }
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum DataValueContext {
    Table, Form, Reception, CompactCard, Summary, CompactCaption, ReceptionTotal
}

/// Pixel-level optical correction for Consolas against Korean unit glyphs.
/// epaint::FontTweak changes only glyph placement, not line boxes, editor
/// carets, button positions or the fixed item-table column rectangles.
/// Consolas digits are optically aligned with Korean unit glyphs using
/// one shared font-level correction. The current calibrated value is +2.5pt.
/// This does not apply to proportional date/telephone text.
pub const DATA_NUMERIC_GLYPH_OFFSET_Y: f32 = 2.5;

pub fn data_value_font(kind:DataValueKind,context:DataValueContext)->FontId {
    let size=match context {
        DataValueContext::Table|DataValueContext::Form
        |DataValueContext::CompactCard=>FONT_BODY,
        DataValueContext::Reception=>FONT_RECEPTION_ITEM_TEXT,
        DataValueContext::Summary=>FONT_SECTION_HEADING,
        DataValueContext::CompactCaption=>FONT_FIELD_LABEL,
        DataValueContext::ReceptionTotal=>18.0,
    };
    match kind {
        DataValueKind::Money|DataValueKind::Quantity=>FontId::monospace(size),
        DataValueKind::Date|DataValueKind::Phone=>FontId::proportional(size),
    }
}

/// A fixed numeric slot right-aligns even the unknown em dash; dates and
/// telephone identifiers are read from the left without numeric right-align.
/// One-line mixed text: preserve the existing 14pt/22pt body label
/// geometry while rendering only the monetary tail with Consolas.
/// Do not split the text into separate widgets: doing so adds egui spacing
/// and moves the following inline action button.
pub fn inline_body_values(parts:&[(&str,Option<DataValueKind>)])->egui::text::LayoutJob {
    let body=spec(Role::Body);
    let regular=egui::TextFormat{
        font_id:FontId::proportional(body.size),
        color:body.color,
        line_height:Some(body.line),
        extra_letter_spacing:body.tracking,
        ..Default::default()
    };
    let mut job=egui::text::LayoutJob::default();
    for &(value,kind) in parts {
        let mut format=regular.clone();
        if let Some(kind)=kind {
            format.font_id=data_value_font(kind,DataValueContext::Form);
        }
        job.append(value,0.0,format);
    }
    job
}

pub fn inline_body_money(prefix:&str,amount:&str)->egui::text::LayoutJob {
    inline_body_values(&[
        (prefix,None),(amount,Some(DataValueKind::Money))
    ])
}

pub fn data_value_align(kind:DataValueKind)->egui::Align {
    match kind {
        DataValueKind::Money|DataValueKind::Quantity=>egui::Align::RIGHT,
        DataValueKind::Date|DataValueKind::Phone=>egui::Align::LEFT,
    }
}

pub fn data_value_text(
    value:&str,kind:DataValueKind,context:DataValueContext
)->RichText {
    let color=if kind==DataValueKind::Quantity{SECONDARY}else{TEXT};
    RichText::new(value).font(data_value_font(kind,context)).color(color)
}

/// Visual offset for management dropdown captions, calibrated against the
/// 34pt trigger's visible glyph bounds rather than the 22pt Body line box.
/// Keep this shared: date-basis, product-filter and freight-group selectors
/// must not acquire separate screen-local Y corrections.
pub const MANAGEMENT_DROPDOWN_TEXT_OPTICAL_Y: f32 = 2.0;
/// Optical adjustment for the TEXT INSIDE compact dropdown popup options.
/// The option's 28pt background, hitbox, hover and selected outline stay put.
pub const MANAGEMENT_DROPDOWN_OPTION_TEXT_OPTICAL_Y: f32 = -1.0;
pub fn management_dropdown_option_text_y(row_height:f32,text_height:f32)->f32 {
    (row_height-text_height)*0.5+MANAGEMENT_DROPDOWN_OPTION_TEXT_OPTICAL_Y
}
/// Date TextEdit optical centering: the original ink appeared too low, while
/// the full -2pt correction appeared too high in subsequent Windows captures.
/// Use the midpoint -1pt without moving control or table rectangles.
pub const MANAGEMENT_DATE_TEXT_OPTICAL_SHIFT_Y: i8 = -1;

/// Common form rows: label glyphs in the 19pt emphasized Korean line box
/// look above the optical center of a 36pt field. Apply the adjustment only
/// to inline *form labels*, never table headers, navigation or body text.
/// The position is shared by every form using components::form_field_label.
pub const FORM_LABEL_OPTICAL_CENTER_Y: f32 = 2.0;
/// Move form-editing ink and its caret together. Never use the Consolas
/// numeric glyph tweak to fix a proportional Body TextEdit baseline.
pub const FORM_INPUT_OPTICAL_SHIFT_Y: i8 = -1;

pub fn form_label_text_y(row_height:f32,galley_height:f32)->f32 {
    (row_height-galley_height)*0.5+FORM_LABEL_OPTICAL_CENTER_Y
}
pub fn form_input_text_margin()->egui::Margin{
    const VERTICAL:i8=2;
    egui::Margin {
        left:8,right:8,
        top:VERTICAL+FORM_INPUT_OPTICAL_SHIFT_Y,
        bottom:VERTICAL-FORM_INPUT_OPTICAL_SHIFT_Y,
    }
}

/// Preserve 2pt average vertical padding, but shift the editable
/// text area's center upward 1pt. Unlike painting text over a TextEdit,
/// this moves the selection cursor and placeholder with the typed text.
pub fn management_date_text_margin() -> egui::Margin {
    const BASE: i8 = 2;
    egui::Margin {
        left: BASE,
        right: BASE,
        top: BASE + MANAGEMENT_DATE_TEXT_OPTICAL_SHIFT_Y,
        bottom: BASE - MANAGEMENT_DATE_TEXT_OPTICAL_SHIFT_Y,
    }
}

/// Compact segmented counters use a neutral vertical text box so the
/// numeric ink and caret stay aligned with the geometric +/- center.
pub fn segmented_count_text_margin()->egui::Margin{
    egui::Margin::symmetric(2,2)
}

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
/// Working-order dealer title: approved 14pt emphasized Korean UI face.
/// Paint into the fixed text slot; never let its natural width reposition the
/// neighboring signal block or management controls.
pub fn workflow_dealer_galley(
    ui:&egui::Ui,text:&str
)->std::sync::Arc<egui::Galley>{
    let role=spec(Role::Subsection);
    let format=egui::TextFormat {
        font_id:FontId::new(
            role.size,FontFamily::Name(EMPHASIS_FAMILY.into())
        ),
        color:TEXT,
        extra_letter_spacing:0.0,
        line_height:Some(role.line),
        ..Default::default()
    };
    ui.fonts_mut(|fonts|fonts.layout_job(
        egui::text::LayoutJob::single_section(text.to_owned(),format)
    ))
}

pub fn body(text: &str) -> RichText { render(text, Role::Body) }
/// Reception source document only: editable/highlighted source, read-only
/// workflow source and OCR review agree at 16pt without enlarging labels,
/// client metadata or other master-table content.
pub fn reception_source_text(text:&str)->RichText {
    body(text).size(FONT_RECEPTION_SOURCE_TEXT)
}
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
/// Controls and temporary notices which share the 14pt management-table
/// text size, but may need their own enabled/disabled colors. Always Regular.
pub fn management_control_text_galley(
    ui:&egui::Ui,text:&str,color:Color32
)->std::sync::Arc<egui::Galley>{
    client_table_galley(ui,text,Role::Body,false,color)
}
pub fn client_table_metadata_galley(
    ui: &egui::Ui, text: &str, color: Color32,
) -> std::sync::Arc<egui::Galley> {
    client_table_galley(ui,text,Role::Metadata,false,color)
}

/// Freight Assignment and other approved management tables share
/// a 14pt emphasized header. Header width and baseline are measured from
/// the same typography role, not screen-local font overrides.
pub fn management_table_header_galley(
    ui: &egui::Ui, text: &str
) -> std::sync::Arc<egui::Galley> {
    client_table_galley(ui,text,Role::Body,true,SECONDARY)
}

/// Keep the original freight call sites as a compatibility entry point.
pub fn freight_table_header_galley(
    ui: &egui::Ui, text: &str
) -> std::sync::Arc<egui::Galley> {
    management_table_header_galley(ui,text)
}


/// Semantic contrast for cells of *repeating management-table rows*.
///
/// Every data cell, INCLUDING billed row totals and other row-level totals,
/// uses the same 14pt Regular face. "Total" is not a reason for bold inside
/// a repeated table. Table headers and standalone KPI/summary cards own
/// their separate emphasis roles and are not rendered by this function.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ManagementTableCellRole {
    ProductName,
    ClientName,
    Text,
    Option,
    Quantity,
    Money,
    RowTotal,
    Category,
    Description,
    Status,
    Muted,
}

impl ManagementTableCellRole {
    // Never promote repeated-row data to a bold font. This intentionally
    // has no per-role branch: emphasis belongs to headers/KPI components.
    const fn emphasized(self) -> bool { false }
}

pub fn management_table_cell_color(role:ManagementTableCellRole)->Color32 {
    match role {
        ManagementTableCellRole::ProductName
        | ManagementTableCellRole::ClientName
        | ManagementTableCellRole::Text
        | ManagementTableCellRole::Option
        | ManagementTableCellRole::Money
        | ManagementTableCellRole::RowTotal => TEXT,
        ManagementTableCellRole::Quantity
        | ManagementTableCellRole::Category
        | ManagementTableCellRole::Description
        | ManagementTableCellRole::Status => SECONDARY,
        ManagementTableCellRole::Muted => MUTED,
    }
}

pub fn management_table_cell_galley(
    ui:&egui::Ui,text:&str,role:ManagementTableCellRole
)->std::sync::Arc<egui::Galley>{
    let numeric=match role {
        ManagementTableCellRole::Money|ManagementTableCellRole::RowTotal=>
            Some(DataValueKind::Money),
        ManagementTableCellRole::Quantity=>Some(DataValueKind::Quantity),
        _=>None,
    };
    if let Some(kind)=numeric {
        let body=spec(Role::Body);
        let format=egui::TextFormat {
            font_id:data_value_font(kind,DataValueContext::Table),
            color:management_table_cell_color(role),
            extra_letter_spacing:body.tracking,
            line_height:Some(body.line),
            ..Default::default()
        };
        ui.fonts_mut(|fonts|fonts.layout_job(
            egui::text::LayoutJob::single_section(text.to_owned(),format)
        ))
    }else{
        client_table_galley(
            ui,text,Role::Body,role.emphasized(),
            management_table_cell_color(role)
        )
    }
}

/// Repeated money cells use the same 14pt family as other business data.
/// Numeric content never inherits KPI bold styling.
pub fn management_table_money_font()->FontId {
    data_value_font(DataValueKind::Money,DataValueContext::Table)
}

/// Order-history shipment details use the same table font with true
/// measured wrapping. The row height is derived from THIS galley, so a long
/// product list never overlaps the next shipment.
pub fn freight_table_wrapped_galley(
    ui:&egui::Ui,text:&str,max_width:f32,
)->std::sync::Arc<egui::Galley>{
    let style=spec(Role::Body);
    let format=egui::TextFormat{
        font_id:FontId::new(style.size,FontFamily::Proportional),
        color:TEXT,
        extra_letter_spacing:style.tracking,
        line_height:Some(style.line),
        ..Default::default()
    };
    let mut job=egui::text::LayoutJob::single_section(text.to_owned(),format);
    job.wrap.max_width=max_width.max(1.0);
    job.wrap.break_anywhere=true;
    ui.fonts_mut(|fonts|fonts.layout_job(job))
}

/// Exact left-anchored category tree name glyphs. Do not place a Label in
/// a full-width allocation: egui may center its text inside that rectangle.
/// Both root and non-root names retain their existing typography role.
pub fn category_tree_name_galley(
    ui:&egui::Ui,text:&str,root:bool
)->std::sync::Arc<egui::Galley>{
    if root {
        client_table_galley(ui,text,Role::Subsection,true,TEXT)
    }else{
        client_table_galley(ui,text,Role::Body,false,TEXT)
    }
}

/// Visual-only breadcrumb shortening. The underlying category path and ID
/// stay complete; only the rendered label may drop ancestors from the LEFT.
/// Prefer keeping whole trailing category names and the final leaf intact.
/// If the leaf alone is wider than the cell, keep its trailing glyphs.
#[derive(Clone,Copy)]
pub enum CategoryPathFont { Body, Metadata, TableCell, FreightTableCell, Subsection }

fn category_path_text_width(ui:&egui::Ui,text:&str,font:CategoryPathFont)->f32{
    // Freight paints muted ancestors and a regular secondary leaf.
    // Measure the same role-specific glyphs as the visible table cell,
    // otherwise the truncated path can end too early or clip its leaf.
    if matches!(font,CategoryPathFont::FreightTableCell){
        let (prefix,leaf)=match text.rfind('>'){
            Some(index) if !text[index+1..].trim().is_empty() =>
                text.split_at(index+1),
            _=>("",text),
        };
        return management_table_cell_galley(
            ui,prefix,ManagementTableCellRole::Muted
        ).size().x + management_table_cell_galley(
            ui,leaf,ManagementTableCellRole::Category
        ).size().x;
    }
    let (role,emphasized,color)=match font{
        CategoryPathFont::Body=>(Role::Body,false,TEXT),
        CategoryPathFont::Metadata=>(Role::Metadata,false,MUTED),
        CategoryPathFont::TableCell=>(Role::TableCell,false,TEXT),
        CategoryPathFont::FreightTableCell=>unreachable!(),
        CategoryPathFont::Subsection=>(Role::Subsection,true,TEXT),
    };
    client_table_galley(ui,text,role,emphasized,color).size().x
}

pub fn fit_category_path(
    ui:&egui::Ui,path:&str,max_width:f32,font:CategoryPathFont
)->String{
    if path.is_empty()||!max_width.is_finite()||max_width<=0.0{
        return String::new();
    }
    if category_path_text_width(ui,path,font)<=max_width{return path.to_owned();}
    let separator=if path.contains(" > "){" > "}
        else if path.contains(" / "){" / "}
        else if path.contains('>'){">"}
        else if path.contains('/') {"/"}else{""};
    if !separator.is_empty(){
        let parts=path.split(separator).map(str::trim).collect::<Vec<_>>();
        // Start with the leaf, then add increasingly many immediate parents.
        // Do not preserve the root at the expense of the destination.
        for start in (1..parts.len()).rev(){
            let candidate=format!("… > {}",parts[start..].join(" > "));
            if category_path_text_width(ui,&candidate,font)<=max_width{
                return candidate;
            }
        }
    }
    let leaf=if separator.is_empty(){path}
        else{path.rsplit(separator).next().unwrap_or(path).trim()};
    for (index,_) in leaf.char_indices(){
        let candidate=format!("…{}",&leaf[index..]);
        if category_path_text_width(ui,&candidate,font)<=max_width{
            return candidate;
        }
    }
    if category_path_text_width(ui,"…",font)<=max_width{"…".into()}
    else{String::new()}
}

pub fn table_cell(text: &str) -> RichText { render(text, Role::TableCell) }
pub fn summary_value(text: &str) -> RichText { render(text, Role::SummaryValue) }

/// Freight overview card label and unit use the same 16pt size as
/// SummaryValue. All three use the shared 23pt line box and a single
/// centerline; the caller owns numeric right alignment.
fn summary_card_secondary_galley(
    ui: &egui::Ui, text: &str
) -> std::sync::Arc<egui::Galley> {
    let style = spec(Role::SummaryValue);
    let format = egui::TextFormat {
        font_id: FontId::new(style.size, FontFamily::Proportional),
        color: SECONDARY,
        extra_letter_spacing: 0.0,
        line_height: Some(style.line),
        ..Default::default()
    };
    ui.fonts_mut(|fonts| fonts.layout_job(
        egui::text::LayoutJob::single_section(text.to_owned(), format)
    ))
}
/// Standalone freight metric number (e.g. 358). Keep the named bold
/// family, independent of the older small one-line SummaryValue role.
pub fn freight_summary_large_number_galley(
    ui: &egui::Ui, text: &str
) -> std::sync::Arc<egui::Galley> {
    let format = egui::TextFormat {
        font_id: FontId::new(
            FONT_FREIGHT_SUMMARY_NUMBER,
            FontFamily::Name(EMPHASIS_FAMILY.into()),
        ),
        color: TEXT,
        line_height: Some(FONT_FREIGHT_SUMMARY_NUMBER + 6.0),
        ..Default::default()
    };
    ui.fonts_mut(|fonts| fonts.layout_job(
        egui::text::LayoutJob::single_section(text.to_owned(), format)
    ))
}

/// Compact label under the large number; both use independent centered rows.
pub fn freight_summary_small_label_galley(
    ui: &egui::Ui, text: &str
) -> std::sync::Arc<egui::Galley> {
    let format = egui::TextFormat {
        font_id: FontId::proportional(FONT_FREIGHT_SUMMARY_LABEL),
        color: SECONDARY,
        line_height: Some(FONT_FREIGHT_SUMMARY_LABEL + 5.0),
        ..Default::default()
    };
    ui.fonts_mut(|fonts| fonts.layout_job(
        egui::text::LayoutJob::single_section(text.to_owned(), format)
    ))
}

pub fn summary_card_label_galley(
    ui: &egui::Ui, text: &str
) -> std::sync::Arc<egui::Galley> {
    summary_card_secondary_galley(ui, text)
}
pub fn summary_card_unit_galley(
    ui: &egui::Ui, text: &str
) -> std::sync::Arc<egui::Galley> {
    summary_card_secondary_galley(ui, text)
}

/// Produce the same summary font as summary_value(), but as measured glyphs
/// for a fixed-position card row without nested automatic label alignment.
pub fn summary_value_galley(
    ui: &egui::Ui, text: &str
) -> std::sync::Arc<egui::Galley> {
    let role = spec(Role::SummaryValue);
    let format = egui::TextFormat {
        font_id: data_value_font(DataValueKind::Money,DataValueContext::Summary),
        color: role.color,
        extra_letter_spacing: role.tracking,
        line_height: Some(role.line),
        ..Default::default()
    };
    ui.fonts_mut(|fonts| fonts.layout_job(
        egui::text::LayoutJob::single_section(text.to_owned(), format)
    ))
}
/// Legacy count caption: keep its original 13pt/20pt appearance while
/// routing the font family through the central quantity policy.
pub fn quantity(text: &str) -> RichText {
    data_value_text(text,DataValueKind::Quantity,DataValueContext::CompactCaption)
        .line_height(Some(20.0))
}

/// Compatibility shim for existing money/quantity editor call sites.
pub fn numeric_editor_font() -> FontId {
    data_value_font(DataValueKind::Money,DataValueContext::Form)
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
        // Tune glyphs at the font level. A visual offset leaves row geometry
        // and selection/caret placement intact across money, quantity, date,
        // phone and compact workflow cards.
        let adjusted=FontData::from_owned(bytes.clone()).tweak(egui::FontTweak {
            y_offset: DATA_NUMERIC_GLYPH_OFFSET_Y,
            ..Default::default()
        });
        fonts.font_data.insert(key.into(), adjusted.into());
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

#[cfg(test)]
mod dropdown_option_text_geometry_tests {
    use super::*;
    #[test]
    fn only_popup_option_text_moves_up_one_point(){
        assert_eq!(MANAGEMENT_DROPDOWN_OPTION_TEXT_OPTICAL_Y,-1.0);
        assert_eq!(management_dropdown_option_text_y(28.0,18.0),4.0);
        assert_eq!(MANAGEMENT_DROPDOWN_TEXT_OPTICAL_Y,2.0);
    }
}

#[cfg(test)]
mod form_baseline_geometry_tests {
    use super::*;
    #[test]
    fn form_label_centers_in_shared_36pt_control_without_changing_size(){
        assert_eq!(form_label_text_y(36.0,19.0),10.5);
        assert_eq!(form_label_text_y(36.0,22.0),9.0);
        let margin=form_input_text_margin();
        assert_eq!(margin.top,1);
        assert_eq!(margin.bottom,3);
        assert_eq!(margin.left,margin.right);
        assert_eq!(margin.top+margin.bottom,4);
        // Data-table numeric alignment is independent of form input centering.
        assert_eq!(DATA_NUMERIC_GLYPH_OFFSET_Y,2.5);
    }
}
#[cfg(test)]
mod management_table_type_roles_tests {
    use super::*;
    #[test]
    fn semantic_data_roles_keep_only_money_and_quantity_monospace(){
        for context in [
            DataValueContext::Table,DataValueContext::Form,
            DataValueContext::Reception,DataValueContext::CompactCard,
            DataValueContext::Summary,DataValueContext::CompactCaption,
            DataValueContext::ReceptionTotal
        ] {
            let size=match context {
                DataValueContext::Table|DataValueContext::Form
                |DataValueContext::CompactCard=>FONT_BODY,
                DataValueContext::Reception=>FONT_RECEPTION_RECIPIENT_TEXT,
                DataValueContext::Summary=>FONT_SECTION_HEADING,
                DataValueContext::CompactCaption=>FONT_FIELD_LABEL,
                DataValueContext::ReceptionTotal=>18.0,
            };
            for kind in [
                DataValueKind::Money,DataValueKind::Quantity,
                DataValueKind::Date,DataValueKind::Phone
            ] {
                let expected=match kind {
                    DataValueKind::Money|DataValueKind::Quantity=>FontId::monospace(size),
                    DataValueKind::Date|DataValueKind::Phone=>FontId::proportional(size),
                };
                assert_eq!(data_value_font(kind,context),expected,
                    "{kind:?} in {context:?} selected the wrong font family");
            }
        }
        assert_eq!(data_value_align(DataValueKind::Money),egui::Align::RIGHT);
        assert_eq!(data_value_align(DataValueKind::Quantity),egui::Align::RIGHT);
        assert_eq!(data_value_align(DataValueKind::Date),egui::Align::LEFT);
        assert_eq!(data_value_align(DataValueKind::Phone),egui::Align::LEFT);
        assert_eq!(DATA_NUMERIC_GLYPH_OFFSET_Y,2.5);
        assert_eq!(numeric_editor_font(),
            data_value_font(DataValueKind::Money,DataValueContext::Form));
        assert_eq!(management_table_money_font(),
            data_value_font(DataValueKind::Money,DataValueContext::Table));
        assert_eq!(spec(Role::SummaryValue).weight as u8,
            Weight::Monospace as u8);
        assert_eq!(data_value_font(
            DataValueKind::Quantity,DataValueContext::CompactCaption
        ),FontId::monospace(FONT_FIELD_LABEL));
        // Phone and date in the reception context share the same font
        // identity as the name, address and source text fields.
        assert_eq!(data_value_font(
            DataValueKind::Phone,DataValueContext::Reception
        ),FontId::proportional(FONT_RECEPTION_RECIPIENT_TEXT));
        assert_eq!(data_value_font(
            DataValueKind::Date,DataValueContext::Form
        ),FontId::proportional(FONT_BODY));
    }

    #[test]
    fn inline_money_uses_two_fonts_in_one_body_height(){
        let job=inline_body_money("709 · ","2,000원");
        assert_eq!(job.sections.len(),2);
        assert_eq!(job.sections[0].format.font_id,FontId::proportional(FONT_BODY));
        assert_eq!(job.sections[1].format.font_id,FontId::monospace(FONT_BODY));
        assert_eq!(job.sections[0].format.line_height,Some(22.0));
        assert_eq!(job.sections[1].format.line_height,Some(22.0));
        assert_eq!(job.text,"709 · 2,000원");
    }

    #[test]
    fn mixed_item_name_quantity_and_price_preserve_one_label(){
        let job=inline_body_values(&[
            ("◆ 709 × ",None),
            ("30",Some(DataValueKind::Quantity)),
            (" · 단가 ",None),
            ("2,000원",Some(DataValueKind::Money)),
        ]);
        assert_eq!(job.sections.len(),4);
        assert_eq!(job.sections[0].format.font_id,FontId::proportional(FONT_BODY));
        assert_eq!(job.sections[1].format.font_id,FontId::monospace(FONT_BODY));
        assert_eq!(job.sections[2].format.font_id,FontId::proportional(FONT_BODY));
        assert_eq!(job.sections[3].format.font_id,FontId::monospace(FONT_BODY));
        assert!(job.sections.iter().all(|section|section.format.line_height==Some(22.0)));
        assert_eq!(job.text,"◆ 709 × 30 · 단가 2,000원");
    }

    #[test]
    fn management_numeric_cell_roles_use_semantic_faces_without_bold(){
        for role in [
            ManagementTableCellRole::Money,ManagementTableCellRole::Quantity,
            ManagementTableCellRole::RowTotal
        ] {
            assert!(!role.emphasized());
        }
        assert_eq!(
            data_value_font(DataValueKind::Money,DataValueContext::Table),
            FontId::monospace(FONT_BODY)
        );
        assert_eq!(management_table_money_font(),FontId::monospace(FONT_BODY));
    }

    #[test]
    fn ordinary_table_data_never_uses_bold_face(){
        for role in [
            ManagementTableCellRole::ProductName,
            ManagementTableCellRole::ClientName,
            ManagementTableCellRole::Text,
            ManagementTableCellRole::Option,
            ManagementTableCellRole::Quantity,
            ManagementTableCellRole::Money,
            ManagementTableCellRole::RowTotal,
            ManagementTableCellRole::Category,
            ManagementTableCellRole::Description,
            ManagementTableCellRole::Status,
            ManagementTableCellRole::Muted,
        ] {
            assert!(!role.emphasized(),"{role:?} must remain Regular");
        }
        assert!(!ManagementTableCellRole::RowTotal.emphasized());
        assert_eq!(management_table_cell_color(ManagementTableCellRole::RowTotal),TEXT);
        assert_eq!(spec(Role::Body).size,FONT_BODY);
        assert_eq!(spec(Role::Body).weight as u8, Weight::Regular as u8);
    }
    #[test]
    fn billable_row_totals_are_not_kpi_totals_at_call_sites(){
        // The same billed amount can appear in an order row and in a
        // settlement row. Both are repeated data, not a standalone KPI.
        let history=include_str!("order_history_manager.rs");
        let settlement=include_str!("daily_settlement_manager.rs");
        assert_eq!(history.matches("ManagementTableCellRole::RowTotal").count(),2);
        assert_eq!(settlement.matches("ManagementTableCellRole::RowTotal").count(),1);
        assert!(!ManagementTableCellRole::RowTotal.emphasized());
    }

    #[test]
    fn management_tables_do_not_reintroduce_bool_emphasis_renderers(){
        for (name,source) in [
            ("freight",include_str!("freight_manager.rs")),
            ("order-history",include_str!("order_history_manager.rs")),
            ("product",include_str!("product_manager.rs")),
            ("excel",include_str!("excel_manager.rs")),
            ("client",include_str!("client_manager.rs")),
            ("daily-settlement",include_str!("daily_settlement_manager.rs")),
            ("common-components",include_str!("components.rs")),
        ] {
            assert!(!source.contains("freight_table_cell_galley("),
                "{name}: old bool-emphasis rendering has returned");
            assert!(!source.contains("client_table_cell_galley("),
                "{name}: old bool-emphasis rendering has returned");
        }
    }

    #[test]
    fn money_quantity_and_row_total_have_shared_contrast(){
        assert_eq!(management_table_cell_color(ManagementTableCellRole::Money),TEXT);
        assert_eq!(management_table_cell_color(ManagementTableCellRole::Quantity),SECONDARY);
        assert_eq!(management_table_cell_color(ManagementTableCellRole::RowTotal),TEXT);
        assert_eq!(management_table_cell_color(ManagementTableCellRole::Category),SECONDARY);
        assert_eq!(management_table_money_font(),numeric_editor_font());
    }
}
