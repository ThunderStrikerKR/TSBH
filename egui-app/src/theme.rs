//! Single source of visual tokens for the egui migration.
//! Approved dual-master SSOT and named exceptions:
//! docs/egui-management-ui-ssot.md (2026-11-21).
//! Historical references: src/ui/design-system.json and
//! docs/egui-ui-design-contract.md.
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
// Disabled management dropdowns use a darker field surface while keeping
// the same height, border and split-chevron geometry as enabled dropdowns.
pub const MANAGEMENT_DROPDOWN_DISABLED_BG: Color32 = Color32::from_rgb(30, 30, 30);
// Strong disabled styling shared by management select and action controls.
// Avoid changing the approved enabled #292929 field / #303030 button roles.
pub const DISABLED_ACTION_BG: Color32 = Color32::from_rgb(30, 30, 30);
pub const DISABLED_ACTION_TEXT: Color32 = Color32::from_rgb(124, 124, 124);
pub const DISABLED_ACTION_BORDER: Color32 = Color32::from_rgb(55, 55, 55);
pub const RAISED: Color32 = Color32::from_rgb(48, 48, 48);
// One component-wide floating feedback style; management screens never define
// independent toast backgrounds, dimensions, type sizes or offsets.
pub const FLOATING_NOTICE_BG: Color32 = RAISED;
pub const FLOATING_NOTICE_MAX_WIDTH: f32 = 320.0;
pub const FLOATING_NOTICE_SCREEN_MARGIN: f32 = 10.0;
pub const FLOATING_NOTICE_ANCHOR_GAP: f32 = SPACE_8;
pub const FLOATING_NOTICE_PADDING_X: i8 = 12;
pub const FLOATING_NOTICE_PADDING_Y: i8 = 8;
pub const FLOATING_NOTICE_ICON_SIZE: f32 = 16.0;
pub const FLOATING_NOTICE_TEXT_SIZE: f32 = typography::FONT_FIELD_LABEL;
pub const FLOATING_NOTICE_CLOSE_HIT_SIZE: f32 = 20.0;
pub const FLOATING_NOTICE_POINTER_HALF_WIDTH: f32 = 6.0;
pub const FLOATING_NOTICE_POINTER_HEIGHT: f32 = 7.0;
pub const FLOATING_NOTICE_SHADOW_OFFSET: [i8;2] = [0,3];
pub const FLOATING_NOTICE_SHADOW_BLUR: u8 = 8;
pub const FLOATING_NOTICE_SHADOW_ALPHA: u8 = 80;
pub const BORDER: Color32 = Color32::from_rgb(80, 80, 80);
pub const DIVIDER: Color32 = Color32::from_rgb(63, 63, 63);
// Stronger than the normal 1pt row divider so fixed table headers have a
// readable bottom boundary, including when their final pixel is clipped.
pub const TABLE_HEADER_OUTLINE: Color32 = Color32::from_rgb(106, 106, 106);
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
// Reception cards are inner content, not a second top-level section.
// Follow the master's soft row divider instead of duplicating PANEL borders.
pub const WORKFLOW_CARD_BORDER: Color32 = DIVIDER;
pub const WORKFLOW_SELECTED_BORDER: Color32 = Color32::from_rgb(245, 158, 11);
pub const WORKFLOW_CARD_PADDING_X: f32 = 9.0;
pub const WORKFLOW_CARD_PADDING_Y: f32 = 8.0;
pub const WORKFLOW_CARD_RADIUS: u8 = 7;
pub const WORKFLOW_SELECTED_STRIPE: f32 = 2.0;
// Actual vertical gap between adjacent order-group cards, not an
// additional gap layered over egui's implicit vertical item spacing.
// Match the 8pt rhythm between independent master cards without enlarging
// the fixed-size signal lamps, buttons or individual order row geometry.
pub const WORKFLOW_CARD_GAP: f32 = SPACE_8;
pub const WORKFLOW_TITLE_HEIGHT: f32 = 34.0;
// At minimum dock width, move Manage/Delete to their own trailing row
// instead of clipping a ten-Hangul-character dealer name.
pub const WORKFLOW_TITLE_MIN_WIDTH: f32 = 184.0;
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
// Product chips remain a distinct metadata layer on BOTH regular and
// alternate rows: muted #373737 fill with the common soft divider border.
// A chip must remain discernible on BOTH the base (#292929) and
// alternate (#303030) row. Reuse the existing muted control-border tier.
pub const WORKFLOW_CHIP_FILL: Color32 = DISABLED_ACTION_BORDER;
pub const WORKFLOW_CHIP_BORDER: Color32 = DIVIDER;
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
// Workflow details use the same subdued, borderless zebra roles as master lists.
pub const WORKFLOW_DETAIL_HEADER_BG: Color32 = RAISED;
// The nested card's base is INPUT (#292929); alternate must be lighter
// just as PANEL -> TABLE_ROW_ALT_BG is lighter in the management master.
pub const WORKFLOW_DETAIL_ALT_BG: Color32 = RAISED;
pub const WORKFLOW_DETAIL_HEADER_HEIGHT: f32 = 28.0;
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
// Make the toolbar's visible upper/lower breathing room symmetric:
// 16pt section inset above, 16pt to the divider below the 36pt toolbar.
// Keep the 4pt space after the 1pt divider separate from this balance.
pub const MANAGEMENT_TOOLBAR_DIVIDER_TOP_GAP: f32 = SECTION_PADDING;
pub const MANAGEMENT_TOOLBAR_DIVIDER_BOTTOM_GAP: f32 = SPACE_4;
// Reception's title row is deliberately compact. Left/right inset stays
// 16pt, but the top/bottom inset is 8pt; these tokens do not affect the
// Settings card or its typography.
pub const RECEPTION_PANE_PADDING_Y: f32 = SPACE_8;
// Source's current-client summary is supporting information beside a
// primary INPUT editor. Use master row/subsection roles, not a second editor.
pub const RECEPTION_CLIENT_SUMMARY_BG: Color32 = TABLE_ROW_ALT_BG;
pub const RECEPTION_CLIENT_SUMMARY_BORDER: Color32 = DIVIDER;
pub const NESTED_PADDING: f32 = SPACE_12;
// The 34pt title row still fully accommodates the standard "+ 새 주문"
// button and other header controls without shortening their labels.
pub const SECTION_HEADER_HEIGHT: f32 = 34.0;
// Recipient navigator in a section title row: same segmented INPUT surface
// and 28pt arrow targets as the Order History date picker, with the current
// recipient name and 1/N progress between arrows. No extra tab/body row.
pub const RECEPTION_RECIPIENT_NAV_ARROW_WIDTH:f32 = MANAGEMENT_DATE_NAV_BUTTON_WIDTH;
pub const RECEPTION_RECIPIENT_NAV_NAME_WIDTH:f32 = 116.0;
pub const RECEPTION_RECIPIENT_NAV_COUNT_WIDTH:f32 = 38.0;
// Clear non-interactive end stops. Their darker face and near-background
// vector chevrons are deliberately lower contrast than active arrows.
pub const RECEPTION_RECIPIENT_NAV_DISABLED_BG:Color32 =
    Color32::from_rgb(27,27,27);
pub const RECEPTION_RECIPIENT_NAV_DISABLED_ICON:Color32 =
    Color32::from_rgb(72,72,72);
pub const RECEPTION_RECIPIENT_NAV_ACTIVE_ICON:Color32 = SECONDARY;
pub const RECEPTION_RECIPIENT_NAV_WIDTH:f32 =
    2.0*RECEPTION_RECIPIENT_NAV_ARROW_WIDTH
    +RECEPTION_RECIPIENT_NAV_NAME_WIDTH
    +RECEPTION_RECIPIENT_NAV_COUNT_WIDTH
    +2.0*FRAME_STROKE_WIDTH;
// Reception titles need the same breathing room as other management headings.
// Body-height accounting consumes this shared token; split rectangles stay put.
pub const SECTION_CONTENT_GAP: f32 = SPACE_8;
// Canvas/footer clearance must be counted in the message editor's height.
pub const RECEPTION_MESSAGE_ACTION_GAP: f32 = SPACE_12;
// One 13pt shortcut label with 3pt keycap top/bottom padding and border.
// The reception intake free-space calculator uses this instead of a plain
// metadata text line, keeping three section groups vertically balanced.
pub const RECEPTION_SHORTCUT_ROW_HEIGHT:f32 = 30.0;
// Passive, low-angle 3D keycap illustrations. The 5pt exposed bottom lip
// makes their underside visible, unlike the flat reception action buttons.
pub const RECEPTION_KEYCAP_HEIGHT:f32 = 27.0;
pub const RECEPTION_KEYCAP_DEPTH:f32 = 5.0;
pub const RECEPTION_KEYCAP_RADIUS:u8 = 4;
pub const RECEPTION_KEYCAP_FACE:Color32 = Color32::from_rgb(82,82,82);
pub const RECEPTION_KEYCAP_SIDE:Color32 = Color32::from_rgb(34,34,34);
pub const RECEPTION_KEYCAP_EDGE:Color32 = Color32::from_rgb(112,112,112);
pub const RECEPTION_KEYCAP_HIGHLIGHT:Color32 = Color32::from_rgb(150,150,150);
pub const RECEPTION_KEYCAP_BOTTOM_HIGHLIGHT:Color32 = Color32::from_rgb(70,70,70);
pub const CARD_PADDING: f32 = SPACE_12;
pub const FIELD_LABEL_WIDTH: f32 = 68.0;
// React recipient detail grid: full-width rows and paired contact cells.
// Three-character labels (휴대폰/메시지) fit with a compact 56pt column.
// All recipient rows share this left-aligned label width.
pub const RECIPIENT_LABEL_WIDTH: f32 = 56.0;
pub const RECIPIENT_PHONE_LABEL_WIDTH: f32 = 32.0;
pub const RECIPIENT_LABEL_GAP: f32 = SPACE_8;
pub const RECIPIENT_ROW_GAP: f32 = SPACE_8;
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
// Shared order-history date toolbar geometry. Keep all controls centered in
// the 36pt row; the date navigation arrows have identical 28pt hit targets.
pub const MANAGEMENT_DATE_NAV_BUTTON_WIDTH: f32 = 28.0;
pub const MANAGEMENT_DATE_CALENDAR_BUTTON_WIDTH: f32 = INPUT_HEIGHT;
pub const MANAGEMENT_DATE_RANGE_SEPARATOR_WIDTH: f32 = SPACE_16;
pub const MANAGEMENT_DATE_CONTROL_GAP: f32 = SPACE_8;
// One geometry source for the shared Order History / Excel calendar popup.
// Its four arrow controls align with the seven equal-sized date columns.
pub const CALENDAR_NAV_SIZE: f32 = MANAGEMENT_DATE_NAV_BUTTON_WIDTH;
pub const CALENDAR_DAY_SIZE: f32 = ACTION_STANDARD_HEIGHT;
pub const CALENDAR_GRID_GAP: f32 = SPACE_4;
pub const CALENDAR_GRID_WIDTH: f32 = 7.0*CALENDAR_DAY_SIZE+6.0*CALENDAR_GRID_GAP;
pub const ACTION_PRIMARY_HEIGHT: f32 = 36.0;
pub const ACTION_STANDARD_HEIGHT: f32 = 34.0;
pub const ACTION_EMPHASIZED_HEIGHT: f32 = 34.0;
pub const ACTION_QUIET_HEIGHT: f32 = 32.0;
// Reserve space for the single-row order total at the bottom of its pane.
pub const SUMMARY_STRIP_HEIGHT: f32 = 58.0;
// The four freight KPIs remain separate cards, each with a centered value
// above its supporting label (not a merged summary strip).
pub const FREIGHT_SUMMARY_CARD_WIDTH: f32 = 228.0;
// Size all geometry from the two text roles: 28pt value, 13pt label.
// Number line 34 + label line 18 + gap 2 + padding 12*2 + border 2 = 80pt.
pub const FREIGHT_SUMMARY_NUMBER_LINE_HEIGHT: f32 =
    typography::FONT_FREIGHT_SUMMARY_NUMBER + 6.0;
pub const FREIGHT_SUMMARY_LABEL_LINE_HEIGHT: f32 =
    typography::FONT_FREIGHT_SUMMARY_LABEL + 5.0;
pub const FREIGHT_SUMMARY_TEXT_GAP: f32 = 2.0;
pub const FREIGHT_SUMMARY_PADDING_X: f32 = 16.0;
pub const FREIGHT_SUMMARY_PADDING_Y: f32 = 12.0;
pub const FREIGHT_SUMMARY_CARD_HEIGHT: f32 =
    FREIGHT_SUMMARY_NUMBER_LINE_HEIGHT + FREIGHT_SUMMARY_LABEL_LINE_HEIGHT
    + FREIGHT_SUMMARY_TEXT_GAP
    + 2.0 * (FREIGHT_SUMMARY_PADDING_Y + FRAME_STROKE_WIDTH);
// Reception items/freight mirrors the React 3-column table. The summary
// remains a separate fixed-height strip below its own scrolling content.
pub const RECEPTION_ITEMS_HEADER_HEIGHT: f32 = 32.0;
pub const RECEPTION_ITEMS_ROW_HEIGHT: f32 = 42.0;
// Reception item table: only product text ellipsizes; quantity and amount
// columns never expand or contract with their values or the pane width.
// On very narrow panes the fixed minimum table scrolls horizontally.
pub const RECEPTION_ITEMS_PRODUCT_MIN_WIDTH: f32 = 180.0;
pub const RECEPTION_ITEMS_QTY_WIDTH: f32 = 76.0; // up to 999개
pub const RECEPTION_ITEMS_AMOUNT_MAX_WIDTH: f32 = 136.0; // up to 99,999,999원
pub const RECEPTION_ITEMS_MIN_TABLE_WIDTH: f32 =
    RECEPTION_ITEMS_PRODUCT_MIN_WIDTH + RECEPTION_ITEMS_QTY_WIDTH
    + RECEPTION_ITEMS_AMOUNT_MAX_WIDTH;
pub const RECEPTION_VAT_ROW_HEIGHT: f32 = 32.0;
pub const RECEPTION_TOTAL_HEIGHT: f32 = 56.0;
pub const HEADER_HEIGHT: f32 = 62.0;
pub const NAV_HEIGHT: f32 = 44.0;
// Both navigation levels remain comfortable click targets; hierarchy comes
// from placement, active colors and the 18pt/16pt typography difference.
pub const MANAGEMENT_TAB_HEIGHT: f32 = 38.0;
// The top of every management page's primary outlined content card aligns
// with Freight > Shipping Group Assignment (the approved reference).
// The offset starts at the management page's content origin, not at the
// window origin, so resizing the Kakao pane never changes its alignment.
// The approved Freight > Shipping Group Assignment uses the ACTUAL
// management-page title inset (16pt total including the 6pt workspace
// gutter), then 8pt title-to-tabs, 12pt tabs-to-KPIs and 16pt KPIs-to-card.
// The three top-content frames also each advance the parent by the default
// 8pt item spacing. Do not revive the older 12pt/16pt pre-title-inset layout.
pub const MANAGEMENT_REFERENCE_HEADING_LINE_HEIGHT: f32 =
    typography::FONT_PAGE_HEADING + 8.0;
pub fn management_primary_card_top_offset(available_width: f32) -> f32 {
    // KPIs are inset 16pt on each side. Wrapped rows must use this same
    // interior width or the reference card jumps at narrow split widths.
    let card_content_width=(available_width-2.0*SECTION_PADDING).max(0.0);
    let cards_per_row = (((card_content_width + SPACE_8) /
        (FREIGHT_SUMMARY_CARD_WIDTH + SPACE_8)).floor() as usize).clamp(1, 4);
    let rows = 4_usize.div_ceil(cards_per_row);
    let summary_height = rows as f32 * FREIGHT_SUMMARY_CARD_HEIGHT
        + rows.saturating_sub(1) as f32 * SPACE_8;
    MANAGEMENT_REFERENCE_HEADING_LINE_HEIGHT
        + (SPACE_16-PANEL_GAP) + SPACE_8
        + MANAGEMENT_TAB_HEIGHT + SPACE_12
        + summary_height + SPACE_16 + SPACING * 3.0
}

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
// Popup chrome uses the same neutral surface hierarchy as the Freight
// Shipping-Group Assignment master. No independent popup color palette.
pub const MODAL_CHROME_BG: Color32 = TABLE_ROW_ALT_BG;
pub const MODAL_SECTION_BG: Color32 = TABLE_ROW_ALT_BG;
pub const MODAL_SECTION_HEADER_BG: Color32 = INPUT;
// Approved OCR review chrome is now the common popup structural master.
// These are visual-role tokens; pane counts and OCR document gutters remain
// content-specific. Changing one master token updates all adopted dialogs.
pub const MODAL_MASTER_WORKSPACE_TOOLBAR_HEIGHT: f32 = 40.0;
pub const MODAL_MASTER_DESCRIPTIVE_HEADER_HEIGHT: f32 = 84.0;
pub const MODAL_MASTER_WORKSPACE_GAP: f32 = SPACE_16;
pub const MODAL_MASTER_CONTENT_INSET: f32 = SPACE_16;
// Compatibility aliases for the earlier OCR review pilot.
pub const MODAL_COMPARISON_HEADER_HEIGHT: f32 =
    MODAL_MASTER_WORKSPACE_TOOLBAR_HEIGHT;
pub const MODAL_REVIEW_HEADER_HEIGHT: f32 =
    MODAL_MASTER_DESCRIPTIVE_HEADER_HEIGHT;
pub const MODAL_REVIEW_GAP: f32 = MODAL_MASTER_WORKSPACE_GAP;
pub const MODAL_REVIEW_CONTENT_INSET: f32 = MODAL_MASTER_CONTENT_INSET;
pub const MODAL_HEADER_HEIGHT: f32 = 56.0;
pub const MODAL_FOOTER_HEIGHT: f32 = 58.0;
pub const MODAL_SECTION_PADDING: f32 = SPACE_12;
pub const MODAL_SECTION_RADIUS: u8 = CONTROL_RADIUS;
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
// React product-admin-redesign.css: fixed 460pt group list / 52pt rows,
// with an independently scrolling editor and fixed action footer.
pub const FAMILY_LIST_WIDTH:f32 = 460.0;
pub const FAMILY_ROW_HEIGHT:f32 = 52.0;
pub const FAMILY_FOOTER_HEIGHT:f32 = 68.0;
pub const MANAGEMENT_TABLE_HEADER_HEIGHT: f32 = 36.0;
/// The bottom 2pt stroke is painted inside the fixed 36pt header,
/// so its text centers in the remaining 34pt, never below the baseline.
pub const MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT:f32=2.0;
/// Single-line management data rows, shared by the approved masters and
/// other compact lists. The 58pt two-line management rows are a different role.
pub const MANAGEMENT_COMPACT_ROW_HEIGHT: f32 = 42.0;
pub const MANAGEMENT_TABLE_ROW_HEIGHT: f32 = 58.0;
// Product table: a full-width, single-line record per product. Wide names
// claim the remaining width while price-editing may scroll horizontally.
pub const PRODUCT_TABLE_ROW_HEIGHT:f32 = MANAGEMENT_COMPACT_ROW_HEIGHT;
pub const PRODUCT_TABLE_MIN_WIDTH:f32 = 1100.0;
pub const PRODUCT_TABLE_NAME_MIN_WIDTH:f32 = 340.0;
pub const PRODUCT_TABLE_OPTION_WIDTH:f32 = 182.0;
pub const PRODUCT_TABLE_PRICE_WIDTH:f32 = 116.0;
// 112pt money field, Save/Cancel buttons and standard cell spacing
// must fit in one fixed-height edited price cell.
pub const PRODUCT_TABLE_EDIT_PRICE_WIDTH:f32 = 264.0;
pub const PRODUCT_TABLE_ACTION_WIDTH:f32 = 100.0;
// Client directory explicitly uses one compact, non-wrapping record per row.
pub const MANAGEMENT_CLIENT_ROW_HEIGHT: f32 = MANAGEMENT_COMPACT_ROW_HEIGHT;
// Text-heavy chooser lists show a primary title and a small status line.
// React automatic-message/product-notice rows use a 48pt compact role.
pub const MANAGEMENT_CHOOSER_ROW_HEIGHT: f32 = 48.0;
// Reception client lookup is a full search-and-compare dialog, not a 460pt
// compact confirmation. Own its geometry here instead of per-screen literals.
pub const CLIENT_CHOOSER_DIALOG_WIDTH: f32 = 760.0;
pub const CLIENT_CHOOSER_DIALOG_HEIGHT: f32 = 700.0;
pub const CLIENT_CHOOSER_ROW_HEIGHT: f32 = 78.0;
pub const CLIENT_CHOOSER_RESULTS_MAX_HEIGHT: f32 = 460.0;
// Dual-master table geometry contract, approved on 2026-11-21.
// Source of truth: table widths, minimums and spare-width distribution live
// here; neither master screen may redefine them in its view implementation.
// The identical HEADER (36pt) and compact parent ROW (42pt) are shared roles,
// while expandable details and toolbar wrapping are EXPLICIT exceptions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagementMasterRowPolicy { Fixed, ExpandableDetails }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagementMasterToolbarPolicy { SingleLine, ResponsiveDateWrap }

#[derive(Clone, Copy, Debug)]
pub struct ManagementMasterTableSpec<const N: usize> {
    pub base_columns: [f32; N],
    pub growth_shares: [f32; N],
    pub min_width: f32,
    pub row_policy: ManagementMasterRowPolicy,
    pub toolbar_policy: ManagementMasterToolbarPolicy,
}

impl<const N: usize> ManagementMasterTableSpec<N> {
    /// Returns per-column widths for a measured DATA viewport, which already
    /// excludes the table scrollbar. Never use it for toolbar-right alignment.
    pub fn widths_for(&self, table_width: f32) -> [f32; N] {
        let extra = (table_width - self.min_width).max(0.0);
        let mut widths = self.base_columns;
        for (width, share) in widths.iter_mut().zip(self.growth_shares) {
            *width += extra * share;
        }
        widths
    }
}

/// Generic management list width allocator. A fixed-header list and its
/// virtual rows must receive the SAME result; data_width excludes scrollbar
/// reservation while the toolbar independently reaches the card's right edge.
pub fn management_weighted_columns<const N:usize>(
    base:[f32;N],shares:[f32;N],data_width:f32,column_gap:f32
)->[f32;N]{
    debug_assert!(column_gap>=0.0 && data_width.is_finite(),
        "invalid management column width parameters");
    debug_assert!(shares.iter().all(|v|*v>=0.0 && v.is_finite()),
        "management spare-space shares cannot be negative");
    debug_assert!((shares.iter().sum::<f32>()-1.0).abs()<0.01,
        "management spare-space shares must sum to 1");
    let min_width=base.iter().sum::<f32>()
        +N.saturating_sub(1) as f32*column_gap;
    let extra=(data_width-min_width).max(0.0);
    let mut widths=base;
    for (width,share) in widths.iter_mut().zip(shares){
        *width+=extra*share;
    }
    widths
}

/// Most content lists assign all remaining width to one text-heavy column.
/// Preserve their individual base widths instead of copying the masters.
pub fn management_single_stretch_columns<const N:usize>(
    base:[f32;N], data_width:f32, column_gap:f32, stretch:usize
)->[f32;N]{
    debug_assert!(stretch<N,"invalid management single-stretch column");
    let mut shares=[0.0;N];
    if let Some(share)=shares.get_mut(stretch){*share=1.0;}
    management_weighted_columns(base,shares,data_width,column_gap)
}

// Freight > Shipping Group Assignment: selection, product, color/option,
// category, shipping group, settings. Fixed 42pt rows; 6 columns + 5 gaps.
// CONFLICT-002: keep the currently approved 202pt product / 144pt settings.
pub const FREIGHT_ASSIGNMENT_MASTER_TABLE: ManagementMasterTableSpec<6> =
    ManagementMasterTableSpec {
        base_columns: [44.0, 202.0, 104.0, 258.0, 150.0, 144.0],
        growth_shares: [0.0, 0.64, 0.0, 0.36, 0.0, 0.0],
        min_width: 942.0,
        row_policy: ManagementMasterRowPolicy::Fixed,
        toolbar_policy: ManagementMasterToolbarPolicy::SingleLine,
    };

// Order History: date, client, shipment/product, management status,
// billed total, payment confirmation, actions. Expandable DETAIL rows
// retain their variable heights; only the parent row uses compact 42pt.
pub const ORDER_HISTORY_MASTER_TABLE: ManagementMasterTableSpec<7> =
    ManagementMasterTableSpec {
        base_columns: [112.0, 156.0, 300.0, 126.0, 124.0, 112.0, 264.0],
        growth_shares: [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
        min_width: 1242.0,
        row_policy: ManagementMasterRowPolicy::ExpandableDetails,
        toolbar_policy: ManagementMasterToolbarPolicy::ResponsiveDateWrap,
    };

// Compatibility aliases: existing consumers continue to receive the
// SAME approved dimensions, never a second independently edited set.
pub const FREIGHT_ASSIGNMENT_COL_WIDTHS: [f32; 6] =
    FREIGHT_ASSIGNMENT_MASTER_TABLE.base_columns;
pub const FREIGHT_ASSIGNMENT_COL_GAP: f32 = SPACE_8;
pub const FREIGHT_BULK_GROUP_WIDTH: f32 = 200.0;
pub const FREIGHT_ASSIGNMENT_ROW_HEIGHT: f32 = MANAGEMENT_COMPACT_ROW_HEIGHT;
pub const FREIGHT_ASSIGNMENT_TABLE_MIN_WIDTH: f32 =
    FREIGHT_ASSIGNMENT_MASTER_TABLE.min_width;
pub const ORDER_HISTORY_COL_WIDTHS: [f32; 7] =
    ORDER_HISTORY_MASTER_TABLE.base_columns;
pub const ORDER_HISTORY_TABLE_MIN_WIDTH: f32 =
    ORDER_HISTORY_MASTER_TABLE.min_width;
// Shipping-group management: four-column grid. The group name gets twice
// its previous width; bundle limit uses only enough space for a short label;
// product membership replaces tariff amounts. The management popup retains
// its compact button column. Keep the old 1265pt table minimum to avoid
// moving scrollbars or the master header edges.
pub const FREIGHT_RATE_COL_WIDTHS: [f32;4] =
    [450.0,112.0,551.0,128.0];
pub const FREIGHT_RATE_TABLE_MIN_WIDTH:f32 = 1265.0;
// Combination and archived group tables have independent information widths.
// Neither inherits the product-assignment 942pt geometry or the rate-edit
// action column; unused space goes first to long identifiers and fee strings.
// Combination history keeps registration/verification metadata in a
// row-anchored "상세정보" popup, not in its compact overview columns.
// The total minimum remains 1229pt (1197pt + four 8pt separators).
pub const FREIGHT_COMBINATION_HISTORY_COL_WIDTHS:[f32;5] =
    [500.0,160.0,160.0,140.0,237.0];
pub const FREIGHT_GROUP_HISTORY_COL_WIDTHS:[f32;6] =
    [190.0,142.0,400.0,150.0,260.0,115.0];

// Daily settlement is two accounting tables, not the six-column freight
// editor: reserve numeric columns before handing the spare width to names
// and receipt memo. Long account details remain available on cell hover.
pub const SETTLEMENT_ORDER_COL_WIDTHS:[f32;8] =
    [258.0,106.0,138.0,148.0,118.0,143.0,205.0,145.0];
pub const SETTLEMENT_PAYMENT_COL_WIDTHS:[f32;7] =
    [172.0,180.0,254.0,141.0,126.0,126.0,260.0];
pub const SETTLEMENT_KPI_CARD_WIDTH:f32 = 278.0;
pub const SETTLEMENT_DATE_FIELD_WIDTH:f32 = 132.0;
pub const MANAGEMENT_TABLE_MIN_WIDTH: f32 = 1320.0;
// The three-column dealer list has its own content minimum (30/38/32).
// Do not force freight/product's 1320pt table width onto the client list.
pub const MANAGEMENT_CLIENT_TABLE_MIN_WIDTH: f32 = 900.0;
// React shipping.css constrains lengthy freight lists to a 60vh scroll
// region; native management tables keep both axes inside that region.
pub const MANAGEMENT_TABLE_VIEWPORT_FRACTION: f32 = 0.60;
pub const MANAGEMENT_SEARCH_MIN_WIDTH: f32 = 240.0;
/// Approved standard width for single-label management filter selectors.
/// Keep large category-path and bulk-action selectors in their separate roles.
pub const MANAGEMENT_FILTER_DROPDOWN_WIDTH: f32 = 148.0;
// Width reserved for the two original client-list actions on one toolbar row.
// Match the actual two button widths plus their shared spacing, rather
// than leaving a large unused gap after the search field.
pub const MANAGEMENT_TOOLBAR_ACTIONS_WIDTH: f32 = 236.0;
// Avoid an excessively stretched client search field in wide windows.
pub const MANAGEMENT_CLIENT_SEARCH_MAX_WIDTH: f32 = 680.0;
// Reserve enough width for a single search field and two unchanged
// right-aligned actions; in a narrow split-pane they stack on two lines.
pub const MANAGEMENT_CLIENT_TOOLBAR_STACK_WIDTH: f32 = 700.0;
/// Two Excel queue actions occupy one trailing slot of the shared 36pt
/// management toolbar. Stack below search only in genuinely narrow panes.
pub const MANAGEMENT_EXCEL_TOOLBAR_ACTIONS_WIDTH: f32 = 330.0;
/// Consistent leading inset for text columns in master headers and rows.
pub const MANAGEMENT_TABLE_TEXT_LEADING_INSET: f32 = 6.0;
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

/// Full-height master table cards keep the same 16pt interior clearance on
/// all four sides. Row rendering ends above the bottom inset; the scrollbar
/// rail is independent and remains visually centered in that inset rather
/// than touching the outer border. Only the two single-table masters use it.
pub const MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING:f32=SECTION_PADDING;
pub fn management_master_table_frame()->Frame{
    section_frame().inner_margin(egui::Margin{
        left:SECTION_PADDING as i8,
        right:SECTION_PADDING as i8,
        top:SECTION_PADDING as i8,
        bottom:MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING as i8,
    })
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
/// The native rail begins at the first row's exact top edge: a 3pt top
/// inset left a visible break beneath the full-width fixed-header border.
/// Keep this zero for all management tables sharing that header role.
pub const MANAGEMENT_TABLE_SCROLL_TRACK_TOP_INSET:f32=0.0;
/// Retain a small clearance at the BOTTOM of the rail within the body
/// viewport. This does not alter row clipping or the card's 16pt padding.
pub const MANAGEMENT_TABLE_SCROLL_TRACK_BOTTOM_INSET:f32=3.0;

/// One bottom extension for EVERY management-table scrollbar. The
/// 3pt optical shortening is canceled and the rail extends 4pt beyond
/// the rows' viewport. This applies to full-height and embedded tables;
/// their viewport height / footer reservation remain independent.
/// All management tables consume this value via components.rs only.
pub const MANAGEMENT_TABLE_SCROLL_RAIL_EXTENSION:f32=
    MANAGEMENT_TABLE_SCROLL_TRACK_BOTTOM_INSET+SPACE_4;

pub fn management_scroll_style() -> egui::style::ScrollStyle {
    let mut style=egui::style::ScrollStyle::solid();
    style.bar_width=10.0;
    style.handle_min_length=44.0;
    style.bar_inner_margin=SPACE_4;
    style.bar_outer_margin=SPACE_4;
    style.foreground_color=true;
    style
}

/// CONFLICT-003=A: table rails use the same 10pt solid style but WITHOUT
/// independent 4pt inner/outer margins. Non-table scroll areas keep the
/// original management_scroll_style until their role is reviewed.
pub fn management_table_scroll_style()->egui::style::ScrollStyle{
    let mut style=management_scroll_style();
    style.bar_inner_margin=0.0;
    style.bar_outer_margin=0.0;
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

/// Shared fixed-minimum-width action, for select triggers that must match
/// the 34pt management controls rather than egui's shorter default ComboBox.
pub fn action_with_min_width(
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
    // Standard management actions have an explicit disabled appearance.
    // egui's generic disabled tint alone was too close to the enabled state.
    // Respect disabled parent Ui scopes as well as the local enabled flag.
    let effective_enabled=enabled && ui.is_enabled();
    let (fill,color,stroke)=if !effective_enabled
        && matches!(role,
            ButtonRole::Standard | ButtonRole::TableSecondary | ButtonRole::Emphasized
        )
    {
        (DISABLED_ACTION_BG,DISABLED_ACTION_TEXT,
         Stroke::new(1.0,DISABLED_ACTION_BORDER))
    }else{(fill,color,stroke)};
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

#[cfg(test)]
mod management_primary_card_geometry_tests {
    use super::*;

    #[test]
    fn approved_master_table_specs_preserve_columns_and_spare_space() {
        let freight = FREIGHT_ASSIGNMENT_MASTER_TABLE;
        let history = ORDER_HISTORY_MASTER_TABLE;
        // Column minima include N-1 of the standard 8pt gaps.
        let freight_min: f32 = freight.base_columns.iter().sum::<f32>()
            + (freight.base_columns.len() - 1) as f32 * FREIGHT_ASSIGNMENT_COL_GAP;
        let history_min: f32 = history.base_columns.iter().sum::<f32>()
            + (history.base_columns.len() - 1) as f32 * SPACE_8;
        assert!((freight_min - freight.min_width).abs() < 0.01);
        assert!((history_min - history.min_width).abs() < 0.01);
        assert!((freight.growth_shares.iter().sum::<f32>() - 1.0).abs() < 0.01);
        assert!((history.growth_shares.iter().sum::<f32>() - 1.0).abs() < 0.01);
        let freight_wide = freight.widths_for(freight.min_width + 100.0);
        assert!((freight_wide[1] - 266.0).abs() < 0.01);
        assert!((freight_wide[3] - 294.0).abs() < 0.01);
        let history_wide = history.widths_for(history.min_width + 100.0);
        assert!((history_wide[2] - 400.0).abs() < 0.01);
        assert_eq!(freight.row_policy, ManagementMasterRowPolicy::Fixed);
        assert_eq!(history.row_policy, ManagementMasterRowPolicy::ExpandableDetails);
        assert_eq!(freight.toolbar_policy, ManagementMasterToolbarPolicy::SingleLine);
        assert_eq!(history.toolbar_policy, ManagementMasterToolbarPolicy::ResponsiveDateWrap);
    }

    #[test]
    fn standard_workspace_matches_freight_primary_card_anchor() {
        // Title + tabs + four single-row KPI cards + their shared spacing.
        assert!((management_primary_card_top_offset(1200.0)-216.0).abs()<0.01);
    }

    #[test]
    fn narrower_workspaces_reserve_space_for_wrapped_freight_kpis() {
        assert!((management_primary_card_top_offset(650.0)-304.0).abs()<0.01);
        assert!((management_primary_card_top_offset(250.0)-480.0).abs()<0.01);
        // A 950pt pane fits four cards before the 16pt + 16pt page inset,
        // but only three inside it; reserve the wrapped second row.
        assert!((management_primary_card_top_offset(950.0)-304.0).abs()<0.01);
        assert!((management_primary_card_top_offset(968.0)-216.0).abs()<0.01);
    }
}
