//! Single source of visual tokens for the egui migration.
//! Based on src/ui/design-system.json and docs/egui-ui-design-contract.md.
//! Keep screen-specific hard-coded colors and geometry out of view code.

use eframe::egui::{
    self, Color32, CornerRadius, Frame, Response, Stroke, Ui, Vec2,
};
use crate::typography;

pub const CANVAS: Color32 = Color32::from_rgb(16, 16, 16);
pub const HEADER_BG: Color32 = Color32::from_rgb(27, 27, 27);
// Level-2 / primary section surface. Darker than INPUT (#292929) so
// repeated order cards remain distinguishable inside the workflow pane.
// This shared token intentionally updates every egui panel using PANEL.
pub const PANEL: Color32 = Color32::from_rgb(24, 24, 24);
// Shared surface level 2.5: subtle alternate rows inside PANEL lists.
// Never use INPUT/RAISED (fields/buttons) as a table zebra fill.
pub const TABLE_ROW_ALT_BG: Color32 = Color32::from_rgb(30, 30, 30);
pub const INPUT: Color32 = Color32::from_rgb(41, 41, 41);
pub const RAISED: Color32 = Color32::from_rgb(48, 48, 48);
pub const BORDER: Color32 = Color32::from_rgb(80, 80, 80);
pub const DIVIDER: Color32 = Color32::from_rgb(63, 63, 63);
pub const TEXT: Color32 = Color32::from_rgb(237, 237, 237);
pub const SECONDARY: Color32 = Color32::from_rgb(185, 185, 185);
pub const MUTED: Color32 = Color32::from_rgb(153, 153, 153);
pub const SELECTION: Color32 = Color32::from_rgb(112, 180, 255);
pub const SELECTED_ORDER: Color32 = Color32::from_rgb(242, 186, 100);
// Workflow cards: original React workflow.css card, not generic navigation tiles.
// The selected stroke color follows the shared selection role.
// Individual orders are nested INPUT cards on the shared PANEL surface.
pub const WORKFLOW_CARD_BG: Color32 = INPUT;
pub const WORKFLOW_SELECTED_BG: Color32 = Color32::from_rgb(36, 36, 36);
pub const WORKFLOW_CARD_BORDER: Color32 = Color32::from_rgb(83, 83, 83);
pub const WORKFLOW_SELECTED_BORDER: Color32 = Color32::from_rgb(245, 158, 11);
pub const WORKFLOW_CARD_PADDING_X: f32 = 9.0;
pub const WORKFLOW_CARD_PADDING_Y: f32 = 8.0;
pub const WORKFLOW_CARD_RADIUS: u8 = 7;
pub const WORKFLOW_SELECTED_STRIPE: f32 = 2.0;
// Actual vertical gap between adjacent order-group cards, not an
// additional gap layered over egui's implicit vertical item spacing.
pub const WORKFLOW_CARD_GAP: f32 = SPACE_4;
pub const WORKFLOW_TITLE_HEIGHT: f32 = 34.0;
pub const WORKFLOW_SIGNAL_WIDTH: f32 = 82.0;
pub const WORKFLOW_SIGNAL_SIZE: f32 = 18.0;
pub const WORKFLOW_SIGNAL_GAP: f32 = 3.0;
pub const WORKFLOW_SIGNAL_DONE: Color32 = Color32::from_rgb(73, 166, 106);
pub const WORKFLOW_SIGNAL_DONE_BORDER: Color32 = Color32::from_rgb(105, 198, 136);
pub const WORKFLOW_SIGNAL_CURRENT: Color32 = Color32::from_rgb(233, 154, 53);
pub const WORKFLOW_SIGNAL_CURRENT_BORDER: Color32 = Color32::from_rgb(245, 183, 96);
pub const WORKFLOW_SIGNAL_PENDING: Color32 = Color32::from_rgb(51, 51, 51);
pub const WORKFLOW_SIGNAL_PENDING_BORDER: Color32 = Color32::from_rgb(85, 85, 85);
pub const WORKFLOW_RECIPIENT_COL: f32 = 64.0;
pub const WORKFLOW_FREIGHT_COL: f32 = 68.0;
pub const WORKFLOW_VAT_TEXT: Color32 = Color32::from_rgb(185, 215, 246);
pub const WORKFLOW_CHIP_FILL: Color32 = Color32::from_rgb(48, 48, 48);
pub const WORKFLOW_CHIP_BORDER: Color32 = Color32::from_rgb(85, 85, 85);
pub const WORKFLOW_CHIP_RADIUS: u8 = 4;
pub const WORKFLOW_CHIP_HEIGHT: f32 = 24.0;
pub const WORKFLOW_CHIP_PAD_X: i8 = 4;
pub const WORKFLOW_CHIP_PAD_Y: i8 = 2;
pub const WORKFLOW_CONTROL_WIDTH: f32 = 140.0;
pub const WORKFLOW_MANAGE_WIDTH: f32 = 52.0;
pub const WORKFLOW_TRASH_WIDTH: f32 = 34.0;
// The three neutral card helpers (관리 / 삭제 / 이전 단계) share the
// Standard 34pt height instead of mixing native 36pt and borderless tiles.
pub const WORKFLOW_AUX_CONTROL_HEIGHT: f32 = ACTION_STANDARD_HEIGHT;
pub const WORKFLOW_ACTION_ROW_HEIGHT: f32 = ACTION_PRIMARY_HEIGHT;
pub const WORKFLOW_TOGGLE_ICON_WIDTH: f32 = 12.0;
pub const WORKFLOW_SUMMARY_LINE_HEIGHT: f32 = 18.0;
pub const SUCCESS: Color32 = Color32::from_rgb(169, 218, 181);
pub const DANGER: Color32 = Color32::from_rgb(255, 180, 180);
pub const BRIGHT: Color32 = Color32::WHITE;
pub const BRIGHT_TEXT: Color32 = Color32::from_rgb(23, 23, 23);
pub const SPACE_4: f32 = 4.0;
pub const SPACE_8: f32 = 8.0;
pub const SETTINGS_HELP_ICON_SIZE: f32 = 22.0;
pub const SETTINGS_TOP_TWO_COL_MIN_WIDTH: f32 = 760.0;
pub const SETTINGS_SENDER_THREE_COL_MIN_WIDTH: f32 = 1120.0;
pub const SETTINGS_GRID_GAP_X: f32 = 16.0;
pub const SETTINGS_TOP_COL_RATIOS: [f32;2] = [1.0,1.4];
pub const SETTINGS_SENDER_COL_RATIOS: [f32;3] = [1.1,1.6,0.8];
// React account-row: bank / number / holder / 112px delete button.
pub const SETTINGS_ACCOUNT_ROW_MIN_WIDTH: f32 = 600.0;
pub const SETTINGS_ACCOUNT_COL_RATIOS: [f32;3] = [1.0,1.3,1.0];
pub const SETTINGS_ACCOUNT_DELETE_WIDTH: f32 = 112.0;
pub const SETTINGS_ACCOUNT_RECOMMENDATION_MIN_WIDTH: f32 = 560.0;
pub const SPACE_12: f32 = 12.0;
pub const SPACE_16: f32 = 16.0;
pub const SPACE_24: f32 = 24.0;
pub const SPACING: f32 = SPACE_8;
pub const HEADER_PADDING_X: i8 = 16;
pub const BRAND_ICON_SIZE: f32 = 38.0;
// Optical alignment of the visible red glyph within the PNG canvas.
// Keep text and navigation on the original header centerline.
pub const BRAND_ICON_OPTICAL_CENTER_Y: f32 = 2.0;
// A small positive gap keeps the wordmark visually separate from the logo.
// This affects only the brand slot, not navigation or header geometry.
pub const BRAND_WORDMARK_GAP: f32 = 3.0;
pub const HEADER_DRAG_STRIP: f32 = 8.0;
pub const SECTION_PADDING: f32 = SPACE_16;
// Reception's title row is deliberately compact. Left/right inset stays
// 16pt, but the top/bottom inset is 8pt; these tokens do not affect the
// Settings card or its typography.
pub const RECEPTION_PANE_PADDING_Y: f32 = SPACE_8;
pub const NESTED_PADDING: f32 = SPACE_12;
// The 34pt title row still fully accommodates the standard "+ 새 주문"
// button and other header controls without shortening their labels.
pub const SECTION_HEADER_HEIGHT: f32 = 34.0;
pub const SECTION_CONTENT_GAP: f32 = SPACE_4;
pub const CARD_PADDING: f32 = SPACE_12;
pub const FIELD_LABEL_WIDTH: f32 = 68.0;
// React recipient detail grid: full-width rows and paired contact cells.
// Three-character labels (휴대폰/메시지) fit with a compact 56pt column.
// All recipient rows share this left-aligned label width.
pub const RECIPIENT_LABEL_WIDTH: f32 = 56.0;
pub const RECIPIENT_PHONE_LABEL_WIDTH: f32 = 32.0;
pub const RECIPIENT_LABEL_GAP: f32 = 6.0;
pub const RECIPIENT_ROW_GAP: f32 = 6.0;
pub const RECIPIENT_PHONE_COLUMN_GAP: f32 = 8.0;
// Minimum phone widths accommodate an 11-digit mobile and shorter landline.
// Both fields grow proportionally to fill the available row instead of leaving
// a dead zone to the right. Shared label/input alignment is preserved.
pub const RECIPIENT_MOBILE_INPUT_WIDTH: f32 = 150.0;
pub const RECIPIENT_PHONE_INPUT_WIDTH: f32 = 120.0;
pub const RECIPIENT_MOBILE_WIDTH_SHARE: f32 = 0.55;
pub const RECIPIENT_PHONE_ROW_MIN_WIDTH: f32 =
    RECIPIENT_LABEL_WIDTH + RECIPIENT_LABEL_GAP
    + RECIPIENT_MOBILE_INPUT_WIDTH + RECIPIENT_PHONE_COLUMN_GAP
    + RECIPIENT_PHONE_LABEL_WIDTH + RECIPIENT_LABEL_GAP
    + RECIPIENT_PHONE_INPUT_WIDTH;
pub const PANEL_GAP: f32 = 6.0;
// Kakao dock toolbar: one compact row (2pt vertical gap around 22pt controls).
// Only its body rect is passed to the Win32 dock.
pub const KAKAO_TOOLBAR_HEIGHT: f32 = 26.0;
pub const KAKAO_CONTROL_HEIGHT: f32 = 22.0;
pub const KAKAO_TOOLBAR_PAD_X: f32 = 4.0;
// Deliberately separate the Kakao identity icon from the mode group;
// mode-to-mode spacing stays at the shared 4pt.
pub const KAKAO_ICON_MODE_GAP: f32 = SPACE_8;
pub const KAKAO_MENU_ICON_SIZE: f32 = 20.0;
pub const KAKAO_ICON_YELLOW: Color32 = Color32::from_rgb(254,229,0);
pub const KAKAO_ICON_INK: Color32 = Color32::from_rgb(40,35,36);
// Splitter hit target stays 6pt; only the painted line is 1pt (2pt on hover).
pub const DOCK_SPLITTER_LINE: f32 = 1.0;
pub const DOCK_SPLITTER_HOVER_LINE: f32 = 2.0;
pub const SOFT_DIVIDER_WIDTH: f32 = 1.0;
pub const FRAME_STROKE_WIDTH: f32 = 1.0;
pub const CONTROL_RADIUS: u8 = 6;
pub const PANEL_RADIUS: u8 = 9;
pub const INPUT_HEIGHT: f32 = 36.0;
pub const ACTION_PRIMARY_HEIGHT: f32 = 36.0;
pub const ACTION_STANDARD_HEIGHT: f32 = 34.0;
pub const ACTION_EMPHASIZED_HEIGHT: f32 = 34.0;
pub const ACTION_QUIET_HEIGHT: f32 = 32.0;
// Reserve space for the single-row order total at the bottom of its pane.
pub const SUMMARY_STRIP_HEIGHT: f32 = 58.0;
// Freight overview cards are left-anchored, content-sized controls rather
// than four equal-width columns spanning the whole management viewport.
// 64pt numeric slot fits five decimal digits (up to 99,999) at the shared
// 16pt monospaced summary font; the 16pt unit slot is fixed independently.
pub const FREIGHT_SUMMARY_CARD_WIDTH: f32 = 228.0;
// Card spacing scales with its 16pt text, not an unrelated fixed margin:
// horizontal inset 1.0em = 16pt, vertical inset 0.75em = 12pt.
// One 1.375em line (22pt), two borders and insets => 48pt overall.
pub const FREIGHT_SUMMARY_TEXT_SIZE: f32 = typography::FONT_SECTION_HEADING;
pub const FREIGHT_SUMMARY_PADDING_X: f32 = FREIGHT_SUMMARY_TEXT_SIZE;
pub const FREIGHT_SUMMARY_PADDING_Y: f32 = FREIGHT_SUMMARY_TEXT_SIZE * 0.75;
pub const FREIGHT_SUMMARY_LINE_HEIGHT: f32 = FREIGHT_SUMMARY_TEXT_SIZE * 1.375;
pub const FREIGHT_SUMMARY_CARD_HEIGHT: f32 =
    FREIGHT_SUMMARY_LINE_HEIGHT + 2.0 * (FREIGHT_SUMMARY_PADDING_Y + FRAME_STROKE_WIDTH);
pub const FREIGHT_SUMMARY_NUMBER_WIDTH: f32 = 64.0;
pub const FREIGHT_SUMMARY_UNIT_WIDTH: f32 = 16.0;
// Reception items/freight mirrors the React 3-column table. The summary
// remains a separate fixed-height strip below its own scrolling content.
pub const RECEPTION_ITEMS_HEADER_HEIGHT: f32 = 32.0;
pub const RECEPTION_ITEMS_ROW_HEIGHT: f32 = 42.0;
pub const RECEPTION_ITEMS_QTY_MAX_WIDTH: f32 = 78.0;
pub const RECEPTION_ITEMS_AMOUNT_MAX_WIDTH: f32 = 108.0;
pub const RECEPTION_VAT_ROW_HEIGHT: f32 = 32.0;
pub const RECEPTION_TOTAL_HEIGHT: f32 = 56.0;
pub const HEADER_HEIGHT: f32 = 62.0;
pub const NAV_HEIGHT: f32 = 44.0;
// Both navigation levels remain comfortable click targets; hierarchy comes
// from placement, active colors and the 18pt/16pt typography difference.
pub const MANAGEMENT_TAB_HEIGHT: f32 = 38.0;
// Original React manual workspace: max 1760, viewport side gap 36,
// source-to-sheet columns 34:66 (.manual-workspace .manual-columns).
pub const MANUAL_DIALOG_MAX_WIDTH: f32 = 1760.0;
pub const MANUAL_DIALOG_SIDE_GAP: f32 = 36.0;
pub const MANUAL_DIALOG_VERTICAL_GAP: f32 = 44.0;
pub const MANUAL_SOURCE_SHARE: f32 = 0.34;
pub const MANUAL_SOURCE_MIN_WIDTH: f32 = 300.0;
pub const MANUAL_SHEET_MIN_WIDTH: f32 = 500.0;
pub const MANUAL_COLUMN_GAP: f32 = SPACE_12;
pub const MANUAL_SEARCH_DIALOG_WIDTH: f32 = MODAL_DEFAULT_MAX_WIDTH;
pub const MANUAL_SEARCH_RESULTS_HEIGHT: f32 = 420.0;
pub const MANUAL_INPUT_WIDTH: f32 = 250.0;
// Shared native modal/table roles. Manual source and spreadsheet-like rows
// inherit these values instead of locally painted one-off geometry.
pub const MODAL_PANEL_PADDING_X: f32 = 18.0;
pub const MODAL_PANEL_PADDING_Y: f32 = 16.0;
pub const MODAL_DECISION_MAX_WIDTH: f32 = 560.0;
pub const MODAL_DEFAULT_MAX_WIDTH: f32 = 880.0;
pub const MODAL_VIEWPORT_GUTTER: f32 = 18.0;
// One shared dim role for the modal stack; a stacked confirmation is lighter.
pub const MODAL_BACKDROP_ALPHA: u8 = 144;
pub const MODAL_NESTED_BACKDROP_ALPHA: u8 = 94;
// Shared management list roles; the client page does not invent row geometry.
// React CategoryManager effective CSS: 43pt rows, 50pt roots,
// 24pt nesting and a 430pt right-side editor overlay.
pub const CATALOG_TREE_ROW_HEIGHT:f32 = 43.0;
pub const CATALOG_ROOT_ROW_HEIGHT:f32 = 50.0;
pub const CATALOG_TREE_INDENT:f32 = 24.0;
// Explorer-style category hierarchy: understated connectors and row rules.
// Keep these separate from the generic table zebra/section colors.
pub const CATALOG_TREE_GUIDE_COLOR:Color32 = Color32::from_rgb(74,74,74);
pub const CATALOG_TREE_ROW_RULE_COLOR:Color32 = Color32::from_rgb(40,40,40);
pub const CATALOG_TREE_GUIDE_WIDTH:f32 = 1.0;
pub const CATALOG_TREE_ROW_RULE_WIDTH:f32 = 1.0;
pub const CATALOG_TREE_FOLDER_WIDTH:f32 = 14.0;
pub const CATALOG_TREE_FOLDER_HEIGHT:f32 = 12.0;
// Keep category search useful without stretching a single field across
// the full management panel width.
pub const CATALOG_SEARCH_WIDTH:f32 = 560.0;
// Category breadcrumb selectors must reserve a predictable width so that
// trailing ancestors are preserved via typography::fit_category_path.
pub const CATEGORY_PATH_PICKER_WIDTH:f32 = 280.0;
pub const CATEGORY_PATH_PICKER_TEXT_PADDING:f32 = 32.0;
// Reserve the right-side change-location action when painting a category
// ancestor path in the editable category detail pane.
pub const CATEGORY_PARENT_ACTION_RESERVE_WIDTH:f32 = 128.0;
pub const CATALOG_EDITOR_WIDTH:f32 = 430.0;
// React product-admin-redesign.css: fixed 460pt group list / 52pt rows,
// with an independently scrolling editor and fixed action footer.
pub const FAMILY_LIST_WIDTH:f32 = 460.0;
pub const FAMILY_ROW_HEIGHT:f32 = 52.0;
pub const FAMILY_FOOTER_HEIGHT:f32 = 68.0;
pub const MANAGEMENT_TABLE_HEADER_HEIGHT: f32 = 36.0;
pub const MANAGEMENT_TABLE_ROW_HEIGHT: f32 = 58.0;
// Product table: a full-width, single-line record per product. Wide names
// claim the remaining width while price-editing may scroll horizontally.
pub const PRODUCT_TABLE_ROW_HEIGHT:f32 = 42.0;
pub const PRODUCT_TABLE_MIN_WIDTH:f32 = 1100.0;
pub const PRODUCT_TABLE_NAME_MIN_WIDTH:f32 = 340.0;
pub const PRODUCT_TABLE_OPTION_WIDTH:f32 = 182.0;
pub const PRODUCT_TABLE_PRICE_WIDTH:f32 = 116.0;
pub const PRODUCT_TABLE_EDIT_PRICE_WIDTH:f32 = 236.0;
pub const PRODUCT_TABLE_ACTION_WIDTH:f32 = 100.0;
// Client directory explicitly uses one compact, non-wrapping record per row.
pub const MANAGEMENT_CLIENT_ROW_HEIGHT: f32 = 42.0;
// Freight assignment is a task-oriented table, not an auto-sized 11-column
// audit grid. Every header and row uses the SAME widths to keep controls
// readable without wrapped names or vertical buttons.
// Assignment table: selection, product, color/option, category, group,
// action. The redundant "그룹 지정" status column is intentionally absent.
// Compact minimum widths preserve one product per row and let the table fit
// in ordinary management panes without an unnecessary horizontal scrollbar.
pub const FREIGHT_ASSIGNMENT_COL_WIDTHS: [f32; 6] =
    // +20% colors: 69 -> 83, +20% category: 215 -> 258.
    // Both gains (-57pt) come entirely from product: 296 -> 239.
    [44.0, 239.0, 83.0, 258.0, 150.0, 128.0];
pub const FREIGHT_ASSIGNMENT_COL_GAP: f32 = SPACE_8;
// Width for the single-line bulk shipping-group selector.
pub const FREIGHT_BULK_GROUP_WIDTH: f32 = 200.0;
pub const FREIGHT_ASSIGNMENT_ROW_HEIGHT: f32 = 42.0;
// Six columns plus five 8pt gaps: 902 + 40 = 942pt.
// Any extra width is shared by product name and category.
pub const FREIGHT_ASSIGNMENT_TABLE_MIN_WIDTH: f32 = 942.0;
pub const MANAGEMENT_TABLE_MIN_WIDTH: f32 = 1320.0;
// The three-column dealer list has its own content minimum (30/38/32).
// Do not force freight/product's 1320pt table width onto the client list.
pub const MANAGEMENT_CLIENT_TABLE_MIN_WIDTH: f32 = 900.0;
// React shipping.css constrains lengthy freight lists to a 60vh scroll
// region; native management tables keep both axes inside that region.
pub const MANAGEMENT_TABLE_VIEWPORT_FRACTION: f32 = 0.60;
pub const MANAGEMENT_SEARCH_MIN_WIDTH: f32 = 240.0;
// Width reserved for the two original client-list actions on one toolbar row.
// Match the actual two button widths plus their shared spacing, rather
// than leaving a large unused gap after the search field.
pub const MANAGEMENT_TOOLBAR_ACTIONS_WIDTH: f32 = 236.0;
// Avoid an excessively stretched client search field in wide windows.
pub const MANAGEMENT_CLIENT_SEARCH_MAX_WIDTH: f32 = 680.0;
// Reserve enough width for a single search field and two unchanged
// right-aligned actions; in a narrow split-pane they stack on two lines.
pub const MANAGEMENT_CLIENT_TOOLBAR_STACK_WIDTH: f32 = 700.0;
// Existing React TierBadge palette; color is display-only, never a pricing rule.
pub const CLIENT_TIER_WHOLESALE_BG: Color32 = Color32::from_rgb(35,76,55);
pub const CLIENT_TIER_WHOLESALE_TEXT: Color32 = Color32::from_rgb(166,237,192);
pub const CLIENT_TIER_SEMI_BG: Color32 = Color32::from_rgb(83,69,28);
pub const CLIENT_TIER_SEMI_TEXT: Color32 = Color32::from_rgb(255,227,147);
pub const CLIENT_TIER_RETAIL_BG: Color32 = Color32::from_rgb(84,44,48);
pub const CLIENT_TIER_RETAIL_TEXT: Color32 = Color32::from_rgb(255,178,185);
pub const CLIENT_TIER_SPECIAL_BG: Color32 = Color32::from_rgb(68,48,92);
pub const CLIENT_TIER_SPECIAL_TEXT: Color32 = Color32::from_rgb(218,181,255);
pub const MANAGEMENT_MODAL_BODY_MAX_HEIGHT: f32 = 670.0;
pub const MANAGEMENT_CLIENT_COLS: [f32; 3] = [0.30, 0.38, 0.32];
pub const MANUAL_TABLE_MIN_WIDTH: f32 = 1180.0;
pub const MANUAL_TABLE_HEADER_HEIGHT: f32 = 36.0;
pub const MANUAL_TABLE_ROW_HEIGHT: f32 = 42.0;
pub const MANUAL_TABLE_DELIVERY_COL: f32 = 82.0;
pub const MANUAL_TABLE_NAME_COL: f32 = 150.0;
pub const MANUAL_TABLE_ADDRESS_COL: f32 = 360.0;
pub const MANUAL_TABLE_PHONE_COL: f32 = 160.0;
pub const MANUAL_TABLE_NOTE_COL: f32 = 260.0;
pub const MANUAL_TABLE_PRODUCT_COL: f32 = 780.0;
pub const MANUAL_TABLE_QUANTITY_COL: f32 = 96.0;
pub const MANUAL_TABLE_ACTION_COL: f32 = 120.0;
pub const MANUAL_SOURCE_PENDING_BG: Color32 = Color32::from_rgb(63, 63, 63);
pub const MANUAL_SOURCE_ASSIGNED_BG: Color32 = Color32::from_rgb(42, 60, 74);

#[derive(Clone, Copy)]
pub enum ButtonRole {
    Primary,
    Standard,
    /// Subdued repeated row action: standard 34pt target, muted ink/surface.
    TableSecondary,
    Emphasized,
    Quiet,
    Danger,
    Navigation,
    ActiveNavigation,
    ManagementTab,
    ActiveManagementTab,
    KakaoControl,
    ActiveKakaoControl,
}

/// Apply the shared level-2.5 alternate-row surface to a table's local UI.
/// Call inside its ScrollArea before egui::Grid::striped(true); do not
/// globally change faint_bg_color because other egui widgets also use it.
pub fn apply_table_row_colors(ui:&mut Ui) {
    ui.style_mut().visuals.faint_bg_color = TABLE_ROW_ALT_BG;
}

pub fn install(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals.dark_mode = true;
    style.visuals.panel_fill = CANVAS;
    style.visuals.window_fill = PANEL;
    style.visuals.faint_bg_color = RAISED;
    style.visuals.extreme_bg_color = INPUT;
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.widgets.noninteractive.bg_fill = PANEL;
    style.visuals.widgets.inactive.bg_fill = RAISED;
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(53, 53, 53);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(61, 61, 61);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    style.visuals.selection.bg_fill = SELECTION.linear_multiply(0.3);
    style.visuals.selection.stroke = Stroke::new(1.0_f32, SELECTION);
    style.spacing.item_spacing = Vec2::new(SPACING, SPACING);
    // Per-role horizontal padding is applied by action() below.
    style.spacing.button_padding = Vec2::new(SPACE_12, SPACE_8);
    ctx.set_style(style);
}

pub fn modal_frame() -> Frame {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(FRAME_STROKE_WIDTH,BORDER))
        .corner_radius(CornerRadius::same(PANEL_RADIUS))
        .inner_margin(egui::Margin {
            left: MODAL_PANEL_PADDING_X as i8,
            right: MODAL_PANEL_PADDING_X as i8,
            top: MODAL_PANEL_PADDING_Y as i8,
            bottom: MODAL_PANEL_PADDING_Y as i8,
        })
}

pub fn modal_nested_frame() -> Frame {
    Frame::new()
        .fill(INPUT)
        .stroke(Stroke::new(FRAME_STROKE_WIDTH,DIVIDER))
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .inner_margin(SPACE_8)
}

pub fn section_frame() -> Frame {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(FRAME_STROKE_WIDTH, BORDER))
        .corner_radius(CornerRadius::same(PANEL_RADIUS))
        .inner_margin(SECTION_PADDING)
}

/// Overview card keeps the shared frame identity with proportional padding.
/// Only the freight KPI's vertical inset differs from standard section cards.
pub fn freight_summary_frame() -> Frame {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(FRAME_STROKE_WIDTH, BORDER))
        .corner_radius(CornerRadius::same(PANEL_RADIUS))
        .inner_margin(egui::Margin::symmetric(
            FREIGHT_SUMMARY_PADDING_X as i8, FREIGHT_SUMMARY_PADDING_Y as i8
        ))
}

/// Management tables use a permanent, non-overlay scrollbar track.
/// Solid scrollbars remain visible while a list has overflow and never
/// collapse into a faint 2pt floating line when the pointer leaves.
/// Reserve the width in layout so row buttons do not sit underneath it.
pub fn management_scroll_style() -> egui::style::ScrollStyle {
    let mut style=egui::style::ScrollStyle::solid();
    style.bar_width=10.0;
    style.handle_min_length=44.0;
    style.bar_inner_margin=SPACE_4;
    style.bar_outer_margin=SPACE_4;
    style.foreground_color=true;
    style
}

/// Search toolbar and table belong to a single management list card.
/// The card provides only its outline; its toolbar owns inner padding,
/// and rows can span the full width without a second 16pt inset.
pub fn management_list_frame() -> Frame {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(FRAME_STROKE_WIDTH, BORDER))
        .corner_radius(CornerRadius::same(PANEL_RADIUS))
        .inner_margin(FRAME_STROKE_WIDTH as i8)
}

/// Identical inset for a management list's search toolbar and its inline
/// loading/read-only/error status. Keeps both aligned without adding an
/// extra bordered card or changing the data table's full-width columns.
pub fn management_list_strip_frame() -> Frame {
    Frame::new().inner_margin(egui::Margin::symmetric(
        SPACE_12 as i8, SPACE_8 as i8
    ))
}

/// The same outer gutter for Reception and every management page:
/// 6pt CANVAS above, right and below; the KakaoTalk reservation stays
/// flush with the left edge. The content/splitter layout is measured
/// INSIDE this inset, so existing saved column ratios and native HWND
/// placement remain based on the actual available workspace.
pub fn workspace_shell_frame() -> Frame {
    Frame::new()
        .fill(CANVAS)
        .stroke(Stroke::NONE)
        .inner_margin(egui::Margin {
            left: 0,
            right: PANEL_GAP as i8,
            top: PANEL_GAP as i8,
            bottom: PANEL_GAP as i8,
        })
}

/// Existing reception call sites retain their named entry point, but both
/// reception and management use one shared gutter definition.
pub fn reception_workspace_frame() -> Frame {
    workspace_shell_frame()
}

/// Return reception panes to the Settings-inspired PANEL card tier.
/// No outer margin: the 6pt splitter hit area is the only reserved gutter.
/// Compensate the 1pt outline in inner margins so 16pt horizontal and 8pt
/// vertical content insets (and the recently compacted title row) survive.
pub fn docked_pane_frame() -> Frame {
    Frame::new()
        .fill(PANEL)
        .stroke(Stroke::new(FRAME_STROKE_WIDTH, BORDER))
        .corner_radius(CornerRadius::same(PANEL_RADIUS))
        .inner_margin(egui::Margin {
            left: (SECTION_PADDING - FRAME_STROKE_WIDTH) as i8,
            right: (SECTION_PADDING - FRAME_STROKE_WIDTH) as i8,
            top: (RECEPTION_PANE_PADDING_Y - FRAME_STROKE_WIDTH) as i8,
            bottom: (RECEPTION_PANE_PADDING_Y - FRAME_STROKE_WIDTH) as i8,
        })
}

/// The reserved KakaoTalk area is also workspace, whether connected or
/// empty. Preserve its full slot geometry for the native HWND docking.
pub fn kakao_slot_frame() -> Frame {
    Frame::new()
        // One palette level above the canvas (#101010 -> #181818).
        // Only the Kakao reservation changes; native HWND geometry is intact.
        .fill(PANEL)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(0))
        .inner_margin(0)
}

/// Border and rounding belong to the reusable source-document surface.
/// The editor's own frame is disabled so its square corners cannot cover
/// the approved rounded rect below the dealer card.
pub fn source_document_frame() -> Frame {
    Frame::new()
        .fill(INPUT)
        .stroke(Stroke::new(FRAME_STROKE_WIDTH, BORDER))
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .inner_margin(SPACE_4)
}

pub fn nested_frame() -> Frame {
    Frame::new()
        .fill(INPUT)
        .stroke(Stroke::new(1.0_f32, DIVIDER))
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .inner_margin(NESTED_PADDING)
}

pub fn action(ui: &mut Ui, label: &str, role: ButtonRole, enabled: bool) -> Response {
    action_with_min_width(ui, label, role, enabled, 0.0)
}

/// Reuse the exact same action design, filling only the column assigned by a
/// shared button-group component. The surrounding pane geometry is untouched.
pub fn action_full_width(
    ui: &mut Ui, label: &str, role: ButtonRole, enabled: bool,
) -> Response {
    let width = ui.available_width();
    action_with_min_width(ui, label, role, enabled, width)
}

fn action_with_min_width(
    ui: &mut Ui, label: &str, role: ButtonRole, enabled: bool, min_width: f32,
) -> Response {
    let (height, fill, color, stroke): (f32, Color32, Color32, Stroke) = match role {
        ButtonRole::Primary => (ACTION_PRIMARY_HEIGHT, Color32::from_rgb(238, 238, 238), BRIGHT_TEXT, Stroke::NONE),
        ButtonRole::Standard => (ACTION_STANDARD_HEIGHT, RAISED, TEXT, Stroke::new(1.0_f32, BORDER)),
        ButtonRole::TableSecondary => (
            ACTION_STANDARD_HEIGHT,
            Color32::from_rgb(35, 35, 35),
            SECONDARY,
            Stroke::new(1.0, DIVIDER),
        ),
        ButtonRole::Emphasized => (ACTION_EMPHASIZED_HEIGHT, BRIGHT, BRIGHT_TEXT, Stroke::NONE),
        ButtonRole::Quiet => (ACTION_QUIET_HEIGHT, Color32::TRANSPARENT, SECONDARY, Stroke::NONE),
        ButtonRole::Danger => (34.0, Color32::from_rgb(84, 44, 48), DANGER, Stroke::NONE),
        ButtonRole::Navigation => (NAV_HEIGHT, Color32::TRANSPARENT, SECONDARY, Stroke::NONE),
        ButtonRole::ActiveNavigation => (NAV_HEIGHT, Color32::from_rgb(41, 72, 92), TEXT, Stroke::NONE),
        ButtonRole::ManagementTab => (MANAGEMENT_TAB_HEIGHT, Color32::TRANSPARENT, SECONDARY, Stroke::NONE),
        ButtonRole::ActiveManagementTab => (MANAGEMENT_TAB_HEIGHT, RAISED, TEXT, Stroke::new(1.0,BORDER)),
        ButtonRole::KakaoControl => (KAKAO_CONTROL_HEIGHT, Color32::TRANSPARENT, SECONDARY, Stroke::NONE),
        ButtonRole::ActiveKakaoControl => (KAKAO_CONTROL_HEIGHT, RAISED, TEXT, Stroke::new(FRAME_STROKE_WIDTH,BORDER)),
    };
    let font_size = match role {
        ButtonRole::Navigation | ButtonRole::ActiveNavigation => typography::FONT_MAIN_NAV,
        ButtonRole::ManagementTab | ButtonRole::ActiveManagementTab =>
            typography::FONT_MANAGEMENT_TAB,
        ButtonRole::KakaoControl | ButtonRole::ActiveKakaoControl =>
            typography::FONT_METADATA,
        ButtonRole::Primary => typography::FONT_PRIMARY_ACTION,
        _ => typography::FONT_STANDARD_ACTION,
    };
    // Scoped spacing prevents one button role from changing the padding of
    // neighboring buttons, fields and navigation controls.
    let padding_x = match role {
        ButtonRole::Primary => 14.0,
        ButtonRole::Quiet => 8.0,
        ButtonRole::Navigation | ButtonRole::ActiveNavigation => 12.0,
        ButtonRole::ManagementTab | ButtonRole::ActiveManagementTab => 12.0,
        ButtonRole::KakaoControl | ButtonRole::ActiveKakaoControl => 7.0,
        _ => 12.0,
    };
    let radius = match role {
        ButtonRole::Navigation | ButtonRole::ActiveNavigation |
        ButtonRole::ManagementTab | ButtonRole::ActiveManagementTab => 5,
        ButtonRole::KakaoControl | ButtonRole::ActiveKakaoControl => 4,
        _ => CONTROL_RADIUS,
    };
    // All light-filled actions (Primary and Emphasized) use regular-weight
    // captions app-wide. Selected tabs and destructive actions retain their
    // distinct typography; size, color, padding and behavior are unchanged.
    let important = matches!(
        role,
        ButtonRole::Danger | ButtonRole::ActiveNavigation |
        ButtonRole::ActiveManagementTab | ButtonRole::ActiveKakaoControl
    );
    ui.scope(|ui| {
        let padding_y = match role {
            ButtonRole::KakaoControl | ButtonRole::ActiveKakaoControl => 2.0,
            ButtonRole::ManagementTab | ButtonRole::ActiveManagementTab => 6.0,
            _ => 6.0,
        };
        ui.spacing_mut().button_padding = Vec2::new(padding_x, padding_y);
        let button = egui::Button::new(
            typography::action_label(label, font_size, important, color)
        )
            .fill(fill)
            .stroke(stroke)
            .corner_radius(CornerRadius::same(radius))
            .min_size(Vec2::new(min_width, height));
        ui.add_enabled(enabled, button)
    }).inner
}

/// Frameless Windows controls share a single 32pt hit area and precise
/// vector marks. Unicode "-", "□" and "×" had different font baselines.
#[derive(Clone, Copy)]
pub enum WindowControl {
    Minimize,
    Maximize,
    Close,
}

pub fn window_control(ui: &mut Ui, control: WindowControl) -> Response {
    let response = ui.scope(|ui| {
        ui.spacing_mut().button_padding = Vec2::ZERO;
        ui.add(
            egui::Button::new("")
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::NONE)
                .min_size(Vec2::splat(ACTION_QUIET_HEIGHT))
        )
    }).inner;
    if response.hovered() {
        ui.painter().rect_filled(response.rect, 4.0, RAISED);
    }
    let center = response.rect.center();
    let ink = if response.hovered() { TEXT } else { SECONDARY };
    let line = Stroke::new(1.35, ink);
    let painter = ui.painter();
    match control {
        WindowControl::Minimize => {
            painter.line_segment(
                [egui::pos2(center.x - 5.0, center.y),
                 egui::pos2(center.x + 5.0, center.y)], line
            );
        }
        WindowControl::Maximize => {
            let left = center.x - 5.0;
            let right = center.x + 5.0;
            let top = center.y - 5.0;
            let bottom = center.y + 5.0;
            for (a, b) in [
                (egui::pos2(left, top), egui::pos2(right, top)),
                (egui::pos2(right, top), egui::pos2(right, bottom)),
                (egui::pos2(right, bottom), egui::pos2(left, bottom)),
                (egui::pos2(left, bottom), egui::pos2(left, top)),
            ] {
                painter.line_segment([a, b], line);
            }
        }
        WindowControl::Close => {
            painter.line_segment(
                [egui::pos2(center.x - 5.0, center.y - 5.0),
                 egui::pos2(center.x + 5.0, center.y + 5.0)], line
            );
            painter.line_segment(
                [egui::pos2(center.x + 5.0, center.y - 5.0),
                 egui::pos2(center.x - 5.0, center.y + 5.0)], line
            );
        }
    }
    response
}

pub fn divider(ui: &mut Ui) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), SOFT_DIVIDER_WIDTH),
        egui::Sense::hover(),
    );
    ui.painter().line_segment(
        [rect.left_top(), rect.right_top()],
        Stroke::new(SOFT_DIVIDER_WIDTH, DIVIDER),
    );
    ui.add_space(4.0);
}

pub fn label_value(ui: &mut Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.add_sized([FIELD_LABEL_WIDTH, INPUT_HEIGHT], egui::Label::new(typography::field_label(label)));
        ui.label(typography::body(value));
    });
}
