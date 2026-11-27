//! Single source of visual tokens for the egui migration.
//! Approved dual-master SSOT and named exceptions:
//! docs/egui-management-ui-ssot.md (2026-11-21).
//! Historical references: src/ui/design-system.json and
//! docs/egui-ui-design-contract.md.
//! Keep screen-specific hard-coded colors and geometry out of view code.

use crate::ui_layout::SharedAllocationExt as _;
use eframe::egui::{
    self, Color32, CornerRadius, Frame, Stroke, Ui, Vec2,
};

pub const CANVAS: Color32 = Color32::from_rgb(16, 16, 16);
pub const HEADER_BG: Color32 = Color32::from_rgb(27, 27, 27);
// Level-2 / primary section surface. Darker than INPUT (#292929) so
// repeated order cards remain distinguishable inside the workflow pane.
// This shared token intentionally updates every egui panel using PANEL.
pub const PANEL: Color32 = Color32::from_rgb(24, 24, 24);
// Shared surface level 2.5: subtle alternate rows inside PANEL lists.
// Never use INPUT/RAISED (fields/buttons) as a table zebra fill.
pub const TABLE_ROW_ALT_BG: Color32 = Color32::from_rgb(30, 30, 30);
pub const INPUT: Color32 = Color32::from_rgb(34, 34, 34);
// Strong disabled styling shared by management select and action controls.
// Avoid changing the approved enabled #292929 field / #303030 button roles.
pub const DISABLED_ACTION_BG: Color32 = Color32::from_rgb(30, 30, 30);
pub const DISABLED_ACTION_TEXT: Color32 = Color32::from_rgb(124, 124, 124);
pub const DISABLED_ACTION_BORDER: Color32 = Color32::from_rgb(55, 55, 55);
pub const RAISED: Color32 = Color32::from_rgb(41, 41, 41);
// One component-wide floating feedback style; management screens never define
// independent toast backgrounds, dimensions, type sizes or offsets.
pub const FLOATING_NOTICE_BG: Color32 = RAISED;
pub const FLOATING_NOTICE_MAX_WIDTH: f32 = 320.0;
pub const FLOATING_NOTICE_SCREEN_MARGIN: f32 = 10.0;
pub const FLOATING_NOTICE_ANCHOR_GAP: f32 = SPACE_8;
pub const FLOATING_NOTICE_PADDING_X: i8 = SPACE_12 as i8;
pub const FLOATING_NOTICE_PADDING_Y: i8 = SPACE_8 as i8;
pub const FLOATING_NOTICE_ICON_SIZE: f32 = 16.0;
pub const FLOATING_NOTICE_TEXT_SIZE: f32 = crate::ui_tokens::FONT_FIELD_LABEL;
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
pub const PLACEHOLDER_TEXT:Color32=Color32::from_gray(96);

/// Information role is independent of typography, widget and business state.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum TextRole {
    Heading,
    Value,
    Label,
    Metadata,
}

/// State is explicit, never inferred from the displayed string or a zero.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum TextState {
    Normal,
    Missing,
    Pending,
    Unconfirmed,
    Disabled,
    Placeholder,
    #[cfg(test)]
    Info,
    ActionRequired,
    Error,
    #[cfg(test)]
    Complete,
}

impl TextState{
    pub const fn value(present:bool)->Self{
        if present{Self::Normal}else{Self::Unconfirmed}
    }
    pub const fn query(ready:bool)->Self{
        if ready{Self::Normal}else{Self::Pending}
    }
    pub const fn optional(present:bool)->Self{
        if present{Self::Normal}else{Self::Missing}
    }
}

pub const fn text_ink(role:TextRole,state:TextState)->Color32{
    match state{
        TextState::Normal=>match role{
            TextRole::Heading|TextRole::Value=>TEXT,
            TextRole::Label=>SECONDARY,
            TextRole::Metadata=>MUTED,
        },
        TextState::Missing|TextState::Pending|TextState::Unconfirmed=>MUTED,
        TextState::Disabled=>DISABLED_ACTION_TEXT,
        TextState::Placeholder=>PLACEHOLDER_TEXT,
        #[cfg(test)]
        TextState::Info=>ReviewTone::Info.text_color(),
        TextState::ActionRequired=>ReviewTone::Action.text_color(),
        TextState::Error=>ReviewTone::Error.text_color(),
        #[cfg(test)]
        TextState::Complete=>ReviewTone::Complete.text_color(),
    }
}
pub const SELECTION: Color32 = Color32::from_rgb(112, 180, 255);
pub const INTERACTION_BLUE_TINT:Color32=Color32::from_rgba_unmultiplied_const(112,180,255,20);
pub const SOURCE_BLOCK_FILL:Color32=Color32::from_rgb(66,66,66);
pub const SOURCE_CONTEXT_FILL:Color32=Color32::from_rgb(73,73,73);
pub const MANUAL_ASSIGNMENT_TINT:Color32=INTERACTION_BLUE_TINT;
pub const MANUAL_ASSIGNMENT_STROKE_WIDTH:f32=1.25;
pub const CARD_BORDER:Color32=DIVIDER;
// Restore the original shared blue pointer feedback; focus stays independent.
pub const BUTTON_HOVER_BG:Color32=Color32::from_rgb(47,62,74);
pub const ACTIVE_NAVIGATION_BG:Color32=Color32::from_rgb(41,72,92);
// Strong pointer feedback belongs to controls, not the subdued table/sort tint.
pub const BUTTON_HOVER_TINT:Color32=Color32::from_rgba_unmultiplied_const(112,180,255,66);
pub const BUTTON_HOVER_LIGHT_BG:Color32=Color32::from_rgb(143,197,255);
pub const BUTTON_HOVER_LIGHT_BORDER:Color32=Color32::from_rgb(35,103,174);
pub const BUTTON_HOVER_BORDER_WIDTH:f32=2.0;
pub const LINKED_PREVIEW_BG:Color32=Color32::from_rgb(23,67,109);
pub const LINKED_PREVIEW_TEXT:Color32=Color32::from_rgb(234,244,255);
pub const LINKED_PREVIEW_BORDER:Color32=Color32::from_rgb(117,181,255);
pub const LINKED_PREVIEW_BORDER_WIDTH:f32=2.0;
pub const REQUIRED_REVIEW_TEXT:Color32=ORANGE_EMPHASIS_TEXT;
pub const REQUIRED_REVIEW_BORDER:Color32=SELECTED_ORDER;
pub const ERROR_BG:Color32=Color32::from_rgb(51,37,37);
pub const ERROR_TEXT:Color32=Color32::from_rgb(240,160,160);
pub const ERROR_BORDER:Color32=Color32::from_rgb(199,107,107);
pub const DANGER_ACTION_BG:Color32=Color32::from_rgb(53,37,37);
pub const DANGER_ACTION_BORDER:Color32=Color32::from_rgb(117,68,68);
// Persistent single-select state uses the existing shared orange palette;
// transient pointer hover remains blue.
pub const MANAGEMENT_DROPDOWN_SELECTED_BG: Color32 = ORANGE_EMPHASIS_BG;
pub const MANAGEMENT_DROPDOWN_HOVER_BG: Color32 = Color32::from_rgb(57, 91, 118);
// Hierarchical category explorer reuses the management dropdown master.
// Exploration is neutral; confirmed filtering uses the existing orange role.
// One fixed popup width independent of the category text/depth.
// 655pt is about 20% wider than the previous 546pt.
pub const CATEGORY_EXPLORER_POPUP_WIDTH:f32=655.0;
// Every category list reserves six common tree rows, without vertical gaps.
// The current parent and its children share that scrollable tree viewport.
pub const CATEGORY_EXPLORER_VISIBLE_ROWS:usize=6;
pub const CATEGORY_EXPLORER_LIST_MAX_HEIGHT:f32=
    CATALOG_TREE_ROW_HEIGHT*CATEGORY_EXPLORER_VISIBLE_ROWS as f32;
pub const CATEGORY_EXPLORER_ANCESTOR_MAX_HEIGHT:f32=144.0;
// Popup footer navigation, breadcrumbs and Apply share the 36pt control baseline.
pub const CATEGORY_EXPLORER_CRUMB_HEIGHT:f32=ACTION_PRIMARY_HEIGHT;
pub const CATEGORY_EXPLORER_SEPARATOR_WIDTH:f32=12.0;
// Fixed standard-action slot, including the arrow and parent-navigation label.
pub const CATEGORY_EXPLORER_BACK_WIDTH:f32=108.0;
pub const CATEGORY_EXPLORER_ANCESTOR_INK:Color32=MUTED;
pub const CATEGORY_EXPLORER_SEPARATOR_INK:Color32=Color32::from_gray(110);
pub const CATEGORY_EXPLORER_OMITTED_WIDTH:f32=36.0;
pub const CATEGORY_EXPLORER_APPLY_WIDTH:f32=70.0;
// React workflow.css and receipt-layout.css amber palette; paint only.
pub const SELECTED_ORDER: Color32 = Color32::from_rgb(245, 158, 11); // #F59E0B
pub const ORANGE_EMPHASIS_BG: Color32 = Color32::from_rgb(59, 43, 27); // #3B2B1B
// Orange confirmation surfaces share a single palette regardless of
// whether the trigger is a persistent review card or a one-touch hover.
pub const ORANGE_EMPHASIS_TEXT:Color32=Color32::from_rgb(255,228,188);
// Checkbox marks are neutral; row/option selection retains its independent orange palette.
pub const CHECKBOX_CHECKED_BG:Color32=RAISED;
pub const CHECKBOX_CHECKED_MARK:Color32=BRIGHT;
pub const CHECKBOX_CHECKED_STROKE_WIDTH:f32=2.0;
pub const ONE_TOUCH_BG:Color32=ORANGE_EMPHASIS_BG;
pub const ONE_TOUCH_TEXT:Color32=ORANGE_EMPHASIS_TEXT;
pub const ONE_TOUCH_BORDER:Color32=SELECTED_ORDER;
pub const ONE_TOUCH_BORDER_WIDTH:f32=2.0;
pub const ONE_TOUCH_TINT:Color32=Color32::from_rgba_unmultiplied_const(245,158,11,18);
// Approved selected shipment row: a single rounded surface, not an
// added left stripe. The enclosing order card retains its own accent.
pub const WORKFLOW_SELECTED_ROW_TEXT:Color32=Color32::from_rgb(244,214,162);
pub const WORKFLOW_SELECTED_ROW_BG:Color32=Color32::from_rgb(48,42,34); // #302A22
pub const WORKFLOW_SELECTED_ROW_BORDER:Color32=Color32::TRANSPARENT;
pub fn workflow_selected_row_fill() -> Color32 {
    WORKFLOW_SELECTED_ROW_BG
}
// Compact working-order cards derive every slot from the actual right pane.
// The horizontal outer inset, inter-control gaps and vertical insets use
// the same 6pt rhythm. Long dealer text cannot move any control.
// Nested order cards use the internal-content tier; no second lower surface.
pub const WORKFLOW_CARD_BG: Color32 = INPUT;
pub const WORKFLOW_SELECTED_BG: Color32 = WORKFLOW_CARD_BG;
pub const WORKFLOW_CARD_BORDER: Color32 = DIVIDER;
pub const WORKFLOW_CARD_PADDING_X: f32 = SPACE_6;
pub const WORKFLOW_CARD_PADDING_Y: f32 = SPACE_8;
pub const WORKFLOW_CARD_RADIUS: u8 = 7;
pub const WORKFLOW_SELECTED_STRIPE: f32 = 2.0;
pub const WORKFLOW_CARD_GAP: f32 = SPACE_8;
// Both compact card bands reserve 6pt above and below their 30pt controls.
// Their equal heights keep upper and lower buttons visually balanced at 1080p
// without moving the fixed-width columns or creating extra list rows.
pub const WORKFLOW_CONTROL_VERTICAL_INSET: f32 = SPACE_6;
pub const WORKFLOW_TITLE_HEIGHT: f32 = 42.0;
pub const WORKFLOW_BOTTOM_HEIGHT: f32 = WORKFLOW_TITLE_HEIGHT;
pub const WORKFLOW_BAND_DIVIDER_HEIGHT: f32 = SOFT_DIVIDER_WIDTH;
pub const WORKFLOW_CARD_SLOT_GAP: f32 = SPACE_6;
pub const WORKFLOW_SIGNAL_WIDTH: f32 = 104.0;
pub const WORKFLOW_SIGNAL_BLOCK_HEIGHT: f32 = 30.0;
pub const WORKFLOW_SIGNAL_BLOCK_BG: Color32 = INPUT;
pub const WORKFLOW_SIGNAL_BLOCK_BORDER: Color32 = DIVIDER;
// Square-ish four-stage indicators; group container remains 104×30pt.
pub const WORKFLOW_SIGNAL_SIZE: f32 = 21.0;
pub const WORKFLOW_SIGNAL_HEIGHT: f32 = 20.0;
pub const WORKFLOW_SIGNAL_RADIUS: f32 = 4.0;
pub const WORKFLOW_SIGNAL_BLOCK_RADIUS:f32=5.0;
pub const WORKFLOW_TITLE_LEADING_INSET:f32=SPACE_2;
pub const WORKFLOW_TITLE_TRAILING_INSET:f32=SPACE_4;
pub const WORKFLOW_SUMMARY_BOTTOM_INSET:f32=7.0;
pub const WORKFLOW_VAT_TOP_INSET:f32=3.0;
pub const WORKFLOW_SIGNAL_GAP: f32 = 3.0;
pub const WORKFLOW_SIGNAL_DONE: Color32 = Color32::from_rgb(73, 166, 106);
pub const WORKFLOW_SIGNAL_DONE_BORDER: Color32 = Color32::from_rgb(105, 198, 136);
pub const WORKFLOW_SIGNAL_CURRENT: Color32 = Color32::from_rgb(233, 154, 53);
pub const WORKFLOW_SIGNAL_CURRENT_BORDER: Color32 = Color32::from_rgb(245, 183, 96);
// Initial order review: keep all four outlines visible with no status fill.
pub const WORKFLOW_SIGNAL_EMPTY: Color32 = Color32::TRANSPARENT;
pub const WORKFLOW_SIGNAL_EMPTY_BORDER: Color32 = DISABLED_ACTION_TEXT;
pub const WORKFLOW_SIGNAL_PENDING: Color32 = Color32::from_rgb(51, 51, 51);
pub const WORKFLOW_SIGNAL_PENDING_BORDER: Color32 = Color32::from_rgb(85, 85, 85);
pub const WORKFLOW_VAT_TEXT: Color32 = Color32::from_rgb(185, 215, 246);
// At the 1920×1080 reference, the right-pane card is ~394pt wide.
// Usable width = 394 - 2 border - (6 left + 6 right) = 380pt.
// Upper: 12 toggle + 164 title + 104 status + 44 manage + 32 delete
//        + 4×6 gap = 380.
// Lower: 44 shipping + 132 price + 154 stage + 32 undo
//        + 3×6 gap = 380.
// Both trailing groups have equal 192pt width:
//   104 + 6 + 44 + 6 + 32 == 154 + 6 + 32.
// This preserves aligned start/end x-coordinates, and EVERY control gap
// (signal ↔ manage, manage ↔ delete, stage ↔ undo) is 6pt.
// On narrow cards only the dealer/price slots shrink; controls stay fixed.
pub const WORKFLOW_CONTROL_WIDTH: f32 = 154.0;
pub const WORKFLOW_MANAGE_WIDTH: f32 = 44.0;
pub const WORKFLOW_TRASH_WIDTH: f32 = 32.0;
pub const WORKFLOW_UNDO_WIDTH: f32 = WORKFLOW_TRASH_WIDTH;
pub const WORKFLOW_SHIPPING_WIDTH: f32 = WORKFLOW_MANAGE_WIDTH;
pub const WORKFLOW_AUX_CONTROL_HEIGHT: f32 = 30.0;
pub const WORKFLOW_ACTION_ROW_HEIGHT: f32 = WORKFLOW_AUX_CONTROL_HEIGHT;
pub const WORKFLOW_TOGGLE_ICON_WIDTH: f32 = 12.0;
// Expanded working-order shipments are a plain list, not a table.
// All rows use one bottom-tier surface; only real hover/selection tint it.
// The header, alternating row fills and product-chip surfaces were removed.
pub const WORKFLOW_DETAIL_ROW_BG: Color32 = PANEL;
pub const WORKFLOW_DETAIL_ROW_BORDER: Color32 = DIVIDER;
pub const WORKFLOW_DETAIL_ROW_PADDING_X: f32 = SPACE_12;
// History recipient-detail blocks are independent of workflow product chips.
// Keep their existing 4pt corner radius even when workflow chips are removed.
pub const ORDER_HISTORY_RECIPIENT_BLOCK_RADIUS: f32 = 4.0;
pub const SUCCESS: Color32 = Color32::from_rgb(169, 218, 181);
pub const DANGER: Color32 = Color32::from_rgb(255, 180, 180);
pub const REQUIRED_INPUT_BORDER:Color32=ERROR_BORDER;
pub const REQUIRED_INPUT_TINT:Color32=Color32::from_rgba_unmultiplied_const(199,107,107,18);
// Product sales status: one palette for dropdown captions, options and table cells.
pub const SALE_STATUS_SELLING: Color32 = Color32::from_rgb(123, 207, 148);
pub const SALE_STATUS_SOLD_OUT: Color32 = Color32::from_rgb(231, 122, 120);
pub const SALE_STATUS_PREPARING: Color32 = Color32::from_rgb(228, 195, 104);
pub const SALE_STATUS_STOPPED: Color32 = Color32::from_rgb(145, 145, 145);
pub const BRIGHT: Color32 = Color32::WHITE;
pub const BRIGHT_TEXT: Color32 = Color32::from_rgb(23, 23, 23);
/// Resting white primary-action palette. Manual unassigned source tokens use 90% of this gray channel.
const LEGACY_SOURCE_REFERENCE_GRAY: u8 = 238;
pub const PRIMARY_ACTION_FILL: Color32 = BRIGHT;
// Compact inline-cell edit inset; shared with the 4pt and 8pt layout tokens.
pub use crate::ui_tokens::{SPACE_2,SPACE_4,SPACE_6,SPACE_8,SPACE_12,SPACE_16,SPACE_24};
pub const SETTINGS_HELP_ICON_SIZE: f32 = 22.0;
// Settings/Basic compresses the form into three shared section cards.
// Above the split threshold, the two information cards remain left of the
// independently scrolling account card. These are logical egui points.
pub const SETTINGS_BASIC_SPLIT_MIN_WIDTH:f32=1040.0;
pub const SETTINGS_BASIC_LEFT_PANE_WIDTH:f32=410.0;
pub const SETTINGS_BASIC_SAVE_RESERVE:f32=50.0;
pub const SETTINGS_OPERATOR_FIELD_WIDTH:f32=112.0;
pub const SETTINGS_SENDER_NAME_WIDTH:f32=164.0;
pub const SETTINGS_SENDER_POSTCODE_WIDTH:f32=106.0;
pub const SETTINGS_SENDER_PHONE_WIDTH:f32=160.0;
pub const SETTINGS_SENDER_ADDRESS_WIDTH:f32=500.0;
// Input boxes have content-appropriate capped widths; they must not
// stretch with a wide Settings panel or become three equal-width columns.
pub const SETTINGS_ACCOUNT_ROW_MIN_WIDTH:f32=490.0;
pub const SETTINGS_ACCOUNT_FIELD_WIDTHS:[f32;3]=[118.0,194.0,112.0];
pub const SETTINGS_ACCOUNT_DELETE_WIDTH:f32=102.0;
pub const SETTINGS_ACCOUNT_PICKER_MAX_WIDTH:f32=278.0;
// Cap only the account rows' scroll viewport, never the whole Settings page.
pub const SETTINGS_ACCOUNT_LIST_MIN_HEIGHT:f32=48.0;
pub const SETTINGS_ACCOUNT_LIST_MAX_HEIGHT:f32=260.0;
pub const SETTINGS_ACCOUNT_PANEL_FIXED_HEIGHT:f32=190.0;
pub const SETTINGS_ACCOUNT_STACKED_EXTRA_HEIGHT:f32=62.0;
pub const SPACING: f32 = SPACE_8;
pub const HEADER_PADDING_X: i8 = SPACE_16 as i8;
pub const BRAND_ICON_SIZE: f32 = 38.0;
// Optical alignment of the visible red glyph within the PNG canvas.
// Keep text and navigation on the original header centerline.
pub const BRAND_ICON_OPTICAL_CENTER_Y: f32 = 2.0;
// A small positive gap keeps the wordmark visually separate from the logo.
// This affects only the brand slot, not navigation or header geometry.
pub const BRAND_WORDMARK_GAP: f32 = 3.0;
pub const HEADER_DRAG_STRIP: f32 = 8.0;
pub const SECTION_PADDING: f32 = SPACE_12;
// One structural section-divider geometry across Reception, management and modal sections.
pub const SECTION_DIVIDER_GAP: f32 = SPACE_12;
pub const SECTION_DIVIDER_HEIGHT: f32 = 2.0 * SECTION_DIVIDER_GAP + SOFT_DIVIDER_WIDTH;
// Reception cards use one 8pt vertical inset on both edges.
pub const RECEPTION_PANE_PADDING_Y: f32 = SPACE_8;
// The source highlight extends 3.5pt beyond glyph ink. Reserve 4pt inside
// the shared document viewport so edge tokens retain their complete border.
pub const SOURCE_DOCUMENT_TEXT_MARGIN: i8 = 4;
// Source's current-client summary is supporting information beside a
// primary INPUT editor. Use master row/subsection roles, not a second editor.
pub const RECEPTION_CLIENT_SUMMARY_BG: Color32 = TABLE_ROW_ALT_BG;
pub const RECEPTION_CLIENT_SUMMARY_BORDER: Color32 = DIVIDER;
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
pub const RECEPTION_RECIPIENT_NAV_WIDTH:f32 =
    2.0*RECEPTION_RECIPIENT_NAV_ARROW_WIDTH
    +RECEPTION_RECIPIENT_NAV_NAME_WIDTH
    +RECEPTION_RECIPIENT_NAV_COUNT_WIDTH
    +2.0*FRAME_STROKE_WIDTH;
// Compact reception header only; ordinary management/modal rules stay 12pt.
pub const RECEPTION_SECTION_DIVIDER_GAP: f32 = SPACE_8;
pub const RECEPTION_SECTION_DIVIDER_HEIGHT: f32 =
    2.0 * RECEPTION_SECTION_DIVIDER_GAP + SOFT_DIVIDER_WIDTH;
// Canvas/footer clearance must be counted in the message editor's height.
pub const RECEPTION_MESSAGE_ACTION_GAP: f32 = RECEPTION_SECTION_DIVIDER_HEIGHT;
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
// React recipient detail grid: full-width rows and paired contact cells.
// Three-character labels (휴대폰/메시지) fit with a compact 56pt column.
// All recipient rows share this left-aligned label width.
pub const RECIPIENT_LABEL_WIDTH: f32 = 56.0;
pub const RECIPIENT_PHONE_LABEL_WIDTH: f32 = 32.0;
pub const RECIPIENT_LABEL_GAP: f32 = SPACE_8;
pub const RECIPIENT_ROW_GAP: f32 = SPACE_8;
pub const RECIPIENT_PHONE_COLUMN_GAP: f32 = SPACE_8;
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
pub const PANEL_GAP: f32 = SPACE_6;
// Kakao dock toolbar: one compact row (2pt vertical gap around 22pt controls).
// Only its body rect is passed to the Win32 dock.
pub const KAKAO_TOOLBAR_HEIGHT: f32 = 26.0;
pub const KAKAO_CONTROL_HEIGHT: f32 = 22.0;
pub const KAKAO_TOOLBAR_PAD_X: f32 = SPACE_4;
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
pub const SELECTION_STROKE_WIDTH:f32=1.0;
pub const REQUIRED_INPUT_STROKE_WIDTH:f32=2.0;
// Radius hierarchy: modal shell 9pt, section cards 8pt, controls 6pt, source chips 4pt.
pub const CONTROL_RADIUS: u8 = 6;
pub const SECTION_RADIUS: u8 = 8;
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
pub const ACTION_EMPHASIZED_HEIGHT: f32 = ACTION_STANDARD_HEIGHT;
pub const ACTION_QUIET_HEIGHT: f32 = 32.0;
// Equal width for the existing read/unread labels in notification rows.
pub const NOTIFICATION_READ_ACTION_WIDTH:f32=3.0*ACTION_STANDARD_HEIGHT;
// Reserve space for the single-row order total at the bottom of its pane.
// The four freight KPIs remain separate cards, each with a centered value
// above its supporting label (not a merged summary strip).
pub const FREIGHT_SUMMARY_CARD_WIDTH: f32 = 228.0;
// Size all geometry from the two text roles: 28pt value, 13pt label.
// Number line 34 + label line 18 + gap 2 + padding 12*2 + border 2 = 80pt.
pub use crate::ui_tokens::{FREIGHT_SUMMARY_NUMBER_LINE_HEIGHT,FREIGHT_SUMMARY_LABEL_LINE_HEIGHT};
pub const FREIGHT_SUMMARY_TEXT_GAP: f32 = SPACE_2;
pub const FREIGHT_SUMMARY_PADDING_X: f32 = SPACE_12;
pub const FREIGHT_SUMMARY_PADDING_Y: f32 = SPACE_12;
pub const FREIGHT_SUMMARY_CARD_HEIGHT: f32 =
    FREIGHT_SUMMARY_NUMBER_LINE_HEIGHT + FREIGHT_SUMMARY_LABEL_LINE_HEIGHT
    + FREIGHT_SUMMARY_TEXT_GAP
    + 2.0 * (FREIGHT_SUMMARY_PADDING_Y + FRAME_STROKE_WIDTH);
// Reception items/freight mirrors the React 3-column table. The summary
// remains a separate fixed-height strip below its own scrolling content.
pub const RECEPTION_ITEMS_HEADER_HEIGHT: f32 = 32.0;
pub const RECEPTION_ITEMS_ROW_HEIGHT: f32 = MANAGEMENT_COMPACT_ROW_HEIGHT;
// Reception item table: only product text ellipsizes; quantity and amount
// columns never expand or contract with their values or the pane width.
// On very narrow panes the fixed minimum table scrolls horizontally.
pub const RECEPTION_ITEMS_PRODUCT_MIN_WIDTH: f32 = 180.0;
pub const RECEPTION_ITEMS_QTY_WIDTH: f32 = 84.0; // left stepper + 999개
pub const RECEPTION_ITEM_REMOVE_WIDTH: f32 = 28.0;
pub const RECEPTION_ITEMS_AMOUNT_MAX_WIDTH: f32 = 136.0; // up to 99,999,999원
pub const RECEPTION_ITEMS_MIN_TABLE_WIDTH: f32 =
    RECEPTION_ITEMS_PRODUCT_MIN_WIDTH + RECEPTION_ITEMS_QTY_WIDTH
    + RECEPTION_ITEMS_AMOUNT_MAX_WIDTH;
// A shipment's items/freight and the order-wide VAT/total are separate cards.
// Single source of truth for section height budgeting (including the rule).
pub const RECEPTION_SECTION_CHROME_HEIGHT: f32 =
    2.0 * RECEPTION_PANE_PADDING_Y
    + SECTION_HEADER_HEIGHT + RECEPTION_SECTION_DIVIDER_HEIGHT;
pub const RECEPTION_RECIPIENT_BODY_HEIGHT: f32 =
    4.0 * INPUT_HEIGHT + 3.0 * RECIPIENT_ROW_GAP;
pub const RECEPTION_RECIPIENT_SECTION_HEIGHT: f32 =
    RECEPTION_SECTION_CHROME_HEIGHT + RECEPTION_RECIPIENT_BODY_HEIGHT;
pub const RECEPTION_ORDER_TOTAL_SECTION_HEIGHT: f32 =
    RECEPTION_SECTION_CHROME_HEIGHT + RECEPTION_TOTAL_HEIGHT;
pub const RECEPTION_ORDER_TOTAL_SECTION_GAP: f32 = SPACE_8;
// VAT modal recipient list is a distinct bounded scroll region with a
// visible surface; always reserve five 42pt rows, grow up to ten before scroll.
pub const RECEPTION_VAT_LIST_MIN_VISIBLE_ROWS: usize = 5;
pub const RECEPTION_VAT_LIST_VISIBLE_ROWS: usize = 10;
pub const RECEPTION_VAT_LIST_ROW_HEIGHT: f32 = MANAGEMENT_COMPACT_ROW_HEIGHT;
pub const RECEPTION_VAT_LIST_SCREEN_RESERVE: f32 = 256.0;

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
// management-page title inset (12pt total including the 6pt workspace
// gutter), then 8pt title-to-tabs, 12pt tabs-to-KPIs and 12pt KPIs-to-card.
// The three top-content frames also each advance the parent by the default
// 8pt item spacing. Do not revive the older 12pt/16pt pre-title-inset layout.
pub const MANAGEMENT_REFERENCE_HEADING_LINE_HEIGHT: f32 =
    crate::ui_tokens::FONT_PAGE_HEADING + 8.0;
pub fn management_primary_card_top_offset(available_width: f32) -> f32 {
    // KPIs are inset 12pt on each side. Wrapped rows must use this same
    // interior width or the reference card jumps at narrow split widths.
    let card_content_width=(available_width-2.0*SECTION_PADDING).max(0.0);
    let cards_per_row = (((card_content_width + SPACE_8) /
        (FREIGHT_SUMMARY_CARD_WIDTH + SPACE_8)).floor() as usize).clamp(1, 4);
    let rows = 4_usize.div_ceil(cards_per_row);
    let summary_height = rows as f32 * FREIGHT_SUMMARY_CARD_HEIGHT
        + rows.saturating_sub(1) as f32 * SPACE_8;
    MANAGEMENT_REFERENCE_HEADING_LINE_HEIGHT
        + (SPACE_12-PANEL_GAP) + SPACE_8
        + MANAGEMENT_TAB_HEIGHT + SPACE_12
        + summary_height + SPACE_12 + SPACING * 3.0
}

// Manual classification uses the live Reception source-pane width rather
// than a separate percentage. The right worksheet receives the remaining
// width and scrolls only when the viewport cannot fit both minimums.
pub const MANUAL_DIALOG_MAX_WIDTH: f32 = 1760.0;
pub const MANUAL_DIALOG_SIDE_GAP: f32 = 36.0;
pub const MANUAL_SOURCE_MIN_WIDTH: f32 = 250.0; // Reception source-pane minimum
pub const MANUAL_SHEET_MIN_WIDTH: f32 = 440.0;
pub const MANUAL_COLUMN_GAP: f32 = SPACE_12;
pub const RECEPTION_ITEMS_TO_FREIGHT_GAP: f32 = SPACE_12;
pub const MANUAL_ITEMS_CARD_MIN_HEIGHT: f32 = 340.0 + (RECEPTION_ITEMS_TO_FREIGHT_GAP - SPACE_8);
// Shared native modal/table roles. Manual source and spreadsheet-like rows
// inherit these values instead of locally painted one-off geometry.
// Popup chrome uses the same neutral surface hierarchy as the Freight
// Shipping-Group Assignment master. No independent popup color palette.
pub const MODAL_SECTION_BG: Color32 = TABLE_ROW_ALT_BG;
// Approved OCR review chrome is now the common popup structural master.
// These are visual-role tokens; pane counts and OCR document gutters remain
// content-specific. Changing one master token updates all adopted dialogs.
pub const MODAL_MASTER_WORKSPACE_TOOLBAR_HEIGHT: f32 = 40.0;
// Clearance from the title divider to the status/action row.
pub const MODAL_MASTER_TOOLBAR_RULE_GAP: f32 = SPACE_12;
// Rebalance the visible outer-frame-to-control and control-to-rule gaps.
pub const MODAL_EDGE_ACTION_SHIFT: f32 = SPACE_8;
pub const MODAL_MASTER_DESCRIPTIVE_HEADER_HEIGHT: f32 = 84.0;
pub const MODAL_MASTER_WORKSPACE_GAP: f32 = SPACE_12;
pub const MODAL_MASTER_CONTENT_INSET: f32 = SPACE_12;
pub const MODAL_HEADER_HEIGHT: f32 = 56.0;
pub const MODAL_FOOTER_HEIGHT: f32 = 58.0;
pub const MODAL_SECTION_PADDING: f32 = SPACE_12;
// Equal clearance above and below the divider in every modal section.
// Kept separate from frame padding so the layout role is explicit.
#[cfg(test)]
pub const MODAL_SECTION_HEADER_RULE_GAP: f32 = SECTION_DIVIDER_GAP;
pub const MODAL_SECTION_RADIUS: u8 = SECTION_RADIUS;
// Approved compact modal form row: label and editable value share one 36pt
// line. No product-management-specific vertical label/input layout.
pub const MODAL_FORM_LABEL_WIDTH: f32 = 80.0;
// Wide form label role for localized captions such as 주 사용 입금계좌.
pub const MODAL_FORM_WIDE_LABEL_WIDTH:f32=128.0;
pub const MODAL_PANEL_PADDING_X: f32 = SPACE_12;
pub const MODAL_PANEL_PADDING_Y: f32 = SPACE_12;
pub const MODAL_DECISION_MAX_WIDTH: f32 = 560.0;
pub const MODAL_DEFAULT_MAX_WIDTH: f32 = 880.0;
// Product detail uses the standard 880pt two-column editor width while
// preserving the fixed 320pt image and the shared Kakao-safe modal master.
pub const PRODUCT_DETAIL_MODAL_WIDTH: f32 = MODAL_DEFAULT_MAX_WIDTH;
pub const PRODUCT_DETAIL_IMAGE_SIZE: f32 = 320.0;
pub const MODAL_VIEWPORT_GUTTER: f32 = 18.0;
// One shared dim role for the modal stack; a stacked confirmation is lighter.
pub const MODAL_BACKDROP_ALPHA: u8 = 144;
pub const MODAL_NESTED_BACKDROP_ALPHA: u8 = 94;
// Shared management list roles; the client page does not invent row geometry.
// CategoryTree master: every depth uses the same row, slots and guide geometry.
pub const CATALOG_TREE_ROW_HEIGHT:f32 = MANAGEMENT_COMPACT_ROW_HEIGHT;
pub const CATALOG_TREE_INDENT:f32 = 24.0;
pub const CATALOG_TREE_INSET:f32 = SPACE_12;
pub const CATEGORY_EXPLORER_CRUMB_TEXT_INSET:f32=SPACE_6;
pub const CATALOG_TREE_CHEVRON_SIZE:f32 = SPACE_16;
pub const CATALOG_TREE_FOLDER_SIZE:f32 = 20.0;
pub const CATALOG_TREE_GAP:f32 = SPACE_8;
pub const CATALOG_TREE_TERMINAL_GAP:f32 = SPACE_4;
pub const CATALOG_TREE_GUIDE_COLOR:Color32 = Color32::from_rgb(74,74,74);
pub const CATALOG_TREE_GUIDE_WIDTH:f32 = 1.0;
pub const CATALOG_TREE_FOLDER_STROKE:f32 = 1.5;
pub const CATALOG_TREE_FOLDER_COLOR:Color32 = SECONDARY;
pub const CATALOG_TREE_SELECTED_INK:Color32 = ORANGE_EMPHASIS_TEXT;
// Keep category search useful without stretching a single field across
// the full management panel width.
pub const CATALOG_SEARCH_WIDTH:f32 = 560.0;
// The category map has no title/help row: search and the two existing
// operations share the same 36pt toolbar. Below this width only, stack them.
pub const CATALOG_MAP_TOOLBAR_ACTIONS_WIDTH:f32 = 260.0;
pub const CATALOG_MAP_TOOLBAR_STACK_WIDTH:f32 =
    CATALOG_MAP_TOOLBAR_ACTIONS_WIDTH+MANAGEMENT_SEARCH_MIN_WIDTH+SPACE_12;
// Category breadcrumb selectors must reserve a predictable width so that
// trailing ancestors are preserved via typography::fit_category_path.
pub const CATEGORY_PATH_PICKER_WIDTH:f32 = 280.0;
// Product-list filter trigger only; do not enlarge the modal category picker
// or the separate 655pt popup. 280pt × 1.30 = 364pt.
pub const PRODUCT_CATEGORY_FILTER_TRIGGER_WIDTH:f32 =
    CATEGORY_PATH_PICKER_WIDTH*1.30;
pub const CATEGORY_PATH_PICKER_TEXT_PADDING:f32 = 32.0;
// Reserve the right-side change-location action when painting a category
// ancestor path in the editable category detail pane.
pub const CATEGORY_PARENT_ACTION_RESERVE_WIDTH:f32 = 128.0;
// Group list width remains independently configurable; rows and footer use shared components.
pub const FAMILY_LIST_WIDTH:f32 = 460.0;
pub const MANAGEMENT_TABLE_HEADER_HEIGHT: f32 = 36.0;
/// The bottom 2pt stroke is painted inside the fixed 36pt header,
/// so its text centers in the remaining 34pt, never below the baseline.
pub const MANAGEMENT_TABLE_HEADER_OUTLINE_HEIGHT:f32=2.0;
/// Single source of truth for compact, single-line data rows across management,
/// reception, catalog trees and manual entry. Two-line rows are a different role.
pub const MANAGEMENT_COMPACT_ROW_HEIGHT: f32 = 42.0;
// Product table: a full-width, single-line record per product. The six
// columns are identical in browse and edit mode; contextual Save/Cancel
// controls float over the list instead of widening its three price columns.
pub const PRODUCT_TABLE_ROW_HEIGHT:f32 = MANAGEMENT_COMPACT_ROW_HEIGHT;
pub const PRODUCT_TABLE_MIN_WIDTH:f32 = 1100.0;
pub const PRODUCT_TABLE_NAME_MIN_WIDTH:f32 = 340.0;
pub const PRODUCT_TABLE_OPTION_WIDTH:f32 = 182.0;
pub const PRODUCT_TABLE_PRICE_WIDTH:f32 = 116.0;
pub const PRODUCT_TABLE_ACTION_WIDTH:f32 = 100.0;
// Client directory explicitly uses one compact, non-wrapping record per row.
pub const MANAGEMENT_CLIENT_ROW_HEIGHT: f32 = MANAGEMENT_COMPACT_ROW_HEIGHT;
// Text-heavy chooser lists show a primary title and a small status line.
// React automatic-message/product-notice rows use a 48pt compact role.
pub const MANAGEMENT_CHOOSER_ROW_HEIGHT: f32 = 48.0;
// Reception client lookup is a full search-and-compare dialog, not a 460pt
// compact confirmation. Own its geometry here instead of per-screen literals.
// The chooser reuses the full 900pt Client Management grid. Reserve enough
// width for four columns, a native scrollbar and the modal's internal padding;
// otherwise the old 760pt dialog forced horizontal scrolling at desktop sizes.
pub const CLIENT_CHOOSER_DIALOG_WIDTH: f32 = 1200.0;
pub const CLIENT_CHOOSER_DIALOG_HEIGHT: f32 = 700.0;
pub const CLIENT_CHOOSER_RESULTS_MAX_HEIGHT: f32 = 460.0;
// Dual-master table geometry contract, approved on 2026-11-21.
// Source of truth: table widths, minimums and spare-width distribution live
// here; neither master screen may redefine them in its view implementation.
// The identical HEADER (36pt) and compact parent ROW (42pt) are shared roles,
// while expandable details and toolbar wrapping are EXPLICIT exceptions.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagementMasterRowPolicy { Fixed, ExpandableDetails }
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManagementMasterToolbarPolicy { SingleLine, ResponsiveDateWrap }

#[derive(Clone, Copy, Debug)]
pub struct ManagementMasterTableSpec<const N: usize> {
    pub base_columns: [f32; N],
    pub growth_shares: [f32; N],
    pub min_width: f32,
    #[cfg(test)]
    pub row_policy: ManagementMasterRowPolicy,
    #[cfg(test)]
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

/// Minimum width is derived from the column definitions and the shared gap.
const fn management_columns_min_width<const N:usize>(base:[f32;N],gap:f32)->f32{
    let mut sum=0.0;
    let mut i=0;
    while i<N{sum+=base[i];i+=1;}
    sum+N.saturating_sub(1) as f32*gap
}
const FREIGHT_ASSIGNMENT_BASE_COLUMNS:[f32;6]=[44.0,202.0,104.0,258.0,150.0,144.0];
const ORDER_HISTORY_BASE_COLUMNS:[f32;7]=[112.0,156.0,300.0,126.0,124.0,112.0,264.0];

// Freight > Shipping Group Assignment: selection, product, color/option,
// category, shipping group, settings. Fixed 42pt rows; 6 columns + 5 gaps.
// CONFLICT-002: keep the currently approved 202pt product / 144pt settings.
pub const FREIGHT_ASSIGNMENT_MASTER_TABLE: ManagementMasterTableSpec<6> =
    ManagementMasterTableSpec {
        base_columns: FREIGHT_ASSIGNMENT_BASE_COLUMNS,
        growth_shares: [0.0, 0.64, 0.0, 0.36, 0.0, 0.0],
        min_width: management_columns_min_width(FREIGHT_ASSIGNMENT_BASE_COLUMNS,FREIGHT_ASSIGNMENT_COL_GAP),
        #[cfg(test)]
        row_policy: ManagementMasterRowPolicy::Fixed,
        #[cfg(test)]
        toolbar_policy: ManagementMasterToolbarPolicy::SingleLine,
    };

// Order History: date, client, shipment/product, management status,
// billed total, payment confirmation, actions. Expandable DETAIL rows
// retain their variable heights; only the parent row uses compact 42pt.
pub const ORDER_HISTORY_MASTER_TABLE: ManagementMasterTableSpec<7> =
    ManagementMasterTableSpec {
        base_columns: ORDER_HISTORY_BASE_COLUMNS,
        growth_shares: [0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0],
        min_width: management_columns_min_width(ORDER_HISTORY_BASE_COLUMNS,SPACE_8),
        #[cfg(test)]
        row_policy: ManagementMasterRowPolicy::ExpandableDetails,
        #[cfg(test)]
        toolbar_policy: ManagementMasterToolbarPolicy::ResponsiveDateWrap,
    };

// Compatibility aliases: existing consumers continue to receive the
// SAME approved dimensions, never a second independently edited set.
pub const FREIGHT_ASSIGNMENT_COL_GAP: f32 = SPACE_8;
pub const FREIGHT_BULK_GROUP_WIDTH: f32 = 200.0;
pub const FREIGHT_ASSIGNMENT_ROW_HEIGHT: f32 = MANAGEMENT_COMPACT_ROW_HEIGHT;
pub const FREIGHT_ASSIGNMENT_TABLE_MIN_WIDTH: f32 =
    FREIGHT_ASSIGNMENT_MASTER_TABLE.min_width;
pub const ORDER_HISTORY_TABLE_MIN_WIDTH: f32 =
    ORDER_HISTORY_MASTER_TABLE.min_width;
// Direct row-entry group management: redistribute the former action column
// and its separator to member names, preserving the existing table minimum.
pub const FREIGHT_RATE_COL_WIDTHS: [f32;3] = [450.0,112.0,687.0];
pub const FREIGHT_RATE_TABLE_MIN_WIDTH:f32 =
    management_columns_min_width(FREIGHT_RATE_COL_WIDTHS,FREIGHT_ASSIGNMENT_COL_GAP);
// Combination details are now opened directly from the row. Redistribute
// the former action column and separator to the combination description.
pub const FREIGHT_COMBINATION_HISTORY_COL_WIDTHS:[f32;4] = [745.0,160.0,160.0,140.0];
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
// Client name, phone and terms stay compact; the address takes remaining
// width. Do not force freight/product's 1320pt grid on the client list.
pub const MANAGEMENT_CLIENT_TABLE_MIN_WIDTH: f32 = 900.0;
pub const MANAGEMENT_CLIENT_TAG_COLUMN_WIDTH:f32=280.0;
pub const MANAGEMENT_CLIENT_TAG_TABLE_MIN_WIDTH:f32=
    MANAGEMENT_CLIENT_TABLE_MIN_WIDTH+MANAGEMENT_CLIENT_TAG_COLUMN_WIDTH;
// React shipping.css constrains lengthy freight lists to a 60vh scroll
// region; native management tables keep both axes inside that region.
pub const MANAGEMENT_TABLE_VIEWPORT_FRACTION: f32 = 0.60;
pub const MANAGEMENT_SEARCH_MIN_WIDTH: f32 = 240.0;
/// Approved standard width for single-label management filter selectors.
/// Keep large category-path and bulk-action selectors in their separate roles.
pub const MANAGEMENT_FILTER_DROPDOWN_WIDTH: f32 = 148.0;
// Common single-select menus scroll locally instead of growing beyond the viewport.
pub const MANAGEMENT_DROPDOWN_MENU_MAX_HEIGHT: f32 = 320.0;
// A 148pt toolbar trigger remains compact, while its searchable popup
// provides enough width for a full search field and single-line options.
pub const MANAGEMENT_SEARCHABLE_POPUP_MIN_WIDTH: f32 = 280.0;
/// Selectable option hit box: 14pt Korean label needs symmetric vertical ink
/// clearance rather than egui's small natural-height selectable row.
pub const MANAGEMENT_DROPDOWN_OPTION_HEIGHT: f32 = 28.0;
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
pub const MANAGEMENT_TABLE_TEXT_LEADING_INSET: f32 = SPACE_6;
pub const MANAGEMENT_TABLE_ACTION_INSET:f32=SPACE_6;
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
// Fixed cells fit a ten-character client name, an 11-digit phone number
// (plus separators), and a tier badge followed by its transaction terms.
pub const MANAGEMENT_CLIENT_NAME_COLUMN_WIDTH:f32 = 172.0;
pub const MANAGEMENT_CLIENT_PHONE_COLUMN_WIDTH:f32 = 154.0;
pub const MANAGEMENT_CLIENT_TERMS_COLUMN_WIDTH:f32 = 350.0;
#[cfg(test)]
pub const MANUAL_TABLE_ROW_HEIGHT: f32 = MANAGEMENT_COMPACT_ROW_HEIGHT;
// Recognized source tokens in Reception and classified manual blocks share
// the same subdued gray 1pt outline. Focused blue evidence keeps its own 2pt stroke.
pub const SOURCE_RECOGNIZED_BORDER: Color32 = Color32::from_rgb(96, 96, 96);
pub const SOURCE_RECOGNIZED_BORDER_WIDTH: f32 = 1.0;
// All source blocks (Reception, manual classification, and linked focus)
// use one geometry regardless of state. Horizontal padding follows text;
// vertical geometry follows the fixed source-document row.
pub const SOURCE_BLOCK_RADIUS: f32 = 4.0;
pub const SOURCE_BLOCK_HORIZONTAL_PADDING: f32 = 1.5;
/// Normal chip leaves 1pt above and below its 22pt row; the focus stroke
/// can extend into this clearance without shifting any text or adjacent row.
pub const SOURCE_BLOCK_ROW_INSET: f32 = 1.0;
pub const SOURCE_BLOCK_ADJACENT_GAP: f32 = 0.5;
// Unassigned source tokens are 10% dimmer than the primary white button.
// Scale the neutral sRGB channel with rounding; never change the button itself.
const MANUAL_SOURCE_UNASSIGNED_GRAY: u8 = ((LEGACY_SOURCE_REFERENCE_GRAY as u16 * 9 + 5) / 10) as u8;
pub const MANUAL_SOURCE_UNASSIGNED_FILL: Color32 = Color32::from_rgb(MANUAL_SOURCE_UNASSIGNED_GRAY, MANUAL_SOURCE_UNASSIGNED_GRAY, MANUAL_SOURCE_UNASSIGNED_GRAY);
pub const MANUAL_SOURCE_UNASSIGNED_TEXT: Color32 = BRIGHT_TEXT;
pub const MANUAL_SOURCE_UNASSIGNED_BORDER: Color32 = MANUAL_SOURCE_UNASSIGNED_FILL;
pub const MANUAL_SOURCE_CLASSIFIED_FILL: Color32 = Color32::from_rgb(48, 48, 48);
pub const MANUAL_SOURCE_CLASSIFIED_TEXT: Color32 = Color32::from_rgb(232, 232, 232);
pub const MANUAL_SOURCE_CLASSIFIED_BORDER: Color32 = SOURCE_RECOGNIZED_BORDER;
pub const MANUAL_SOURCE_BLOCK_BORDER_WIDTH: f32 = SOURCE_RECOGNIZED_BORDER_WIDTH;


#[derive(Clone, Copy)]
pub enum ButtonRole {
    Primary,
    Standard,
    /// Subdued repeated row action: standard 34pt target, muted ink/surface.
    TableSecondary,
    Emphasized,
    Quiet,
    /// Historical transparent action palette, retained only for its regression fixture.
    #[cfg(test)]
    Ghost,
    Danger,
    Navigation,
    ActiveNavigation,
    ManagementTab,
    ActiveManagementTab,
    KakaoControl,
    ActiveKakaoControl,
}


/// Visual state is supplied by the component, never inferred from its caption.
/// Geometry and hit testing stay with the component. Hover/focus overlays
/// remain independently composable through the shared highlight module.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum VisualState{Normal,Disabled,Hovered,Focused,Selected}
impl VisualState{
    pub(crate) const fn enabled(enabled:bool)->Self{
        if enabled{Self::Normal}else{Self::Disabled}
    }
    pub(crate) const fn pointer(enabled:bool,hovered:bool)->Self{
        if !enabled{Self::Disabled}else if hovered{Self::Hovered}else{Self::Normal}
    }
}

/// Checkbox selection uses a neutral mark, not the orange row selection palette.
#[derive(Clone,Copy,Debug,PartialEq)]
pub(crate) struct CheckboxPalette {
    pub fill:Color32,pub outline:Stroke,pub mark:Stroke,
}
pub(crate) fn checkbox_palette(checked:bool,state:VisualState)->Option<CheckboxPalette>{
    // Unchecked and disabled widgets retain native egui appearance and accessibility.
    if !checked || state==VisualState::Disabled{return None;}
    let outline=match state{
        VisualState::Hovered=>Stroke::new(BUTTON_HOVER_BORDER_WIDTH,SELECTION),
        _=>Stroke::new(FRAME_STROKE_WIDTH,BORDER),
    };
    Some(CheckboxPalette{
        fill:CHECKBOX_CHECKED_BG,outline,
        mark:Stroke::new(CHECKBOX_CHECKED_STROKE_WIDTH,CHECKBOX_CHECKED_MARK),
    })
}

/// Scroll rail geometry is a semantic role, not a screen-local style.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum ScrollRailRole{Standard,Table}
#[derive(Clone,Copy,Debug,PartialEq)]
pub(crate) struct ScrollRailGeometry{
    pub width:f32,pub minimum_handle:f32,pub inner_margin:f32,pub outer_margin:f32,
}
pub(crate) const fn scroll_rail_geometry(role:ScrollRailRole)->ScrollRailGeometry{
    let margin=match role{ScrollRailRole::Standard=>SPACE_4,ScrollRailRole::Table=>0.0};
    ScrollRailGeometry{width:10.0,minimum_handle:44.0,inner_margin:margin,outer_margin:margin}
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum SurfaceRole{
    Workspace,Header,Panel,Section,Summary,Input,Inset,Modal,ModalSection,
    AlternateRow,Button,WorkflowCard,FieldAction,NavigationSegment,TableHeader,RaisedInset,Notice,
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) struct SurfacePalette{pub fill:Color32,pub border:Color32}

pub(crate) const fn surface_palette(role:SurfaceRole,state:VisualState)->SurfacePalette{
    use SurfaceRole::{
        Workspace, Header, Panel, Section, Summary, Modal, Input, FieldAction, Inset,
        ModalSection, AlternateRow, Button, TableHeader, RaisedInset, Notice, WorkflowCard,
        NavigationSegment,
    };
    let (fill,border)=match role{
        Workspace=>(CANVAS,Color32::TRANSPARENT),
        Header=>(HEADER_BG,Color32::TRANSPARENT),
        Panel=>(PANEL,Color32::TRANSPARENT),
        Section=>(PANEL,CARD_BORDER),
        Summary|Modal=>(PANEL,BORDER),
        Input|FieldAction=>(INPUT,BORDER),
        Inset=>(INPUT,DIVIDER),
        ModalSection=>(MODAL_SECTION_BG,DIVIDER),
        AlternateRow=>(TABLE_ROW_ALT_BG,Color32::TRANSPARENT),
        Button|TableHeader=>(RAISED,BORDER),
        RaisedInset=>(RAISED,DIVIDER),
        Notice=>(FLOATING_NOTICE_BG,BORDER),
        WorkflowCard=>(WORKFLOW_CARD_BG,WORKFLOW_CARD_BORDER),
        NavigationSegment=>(INPUT,Color32::TRANSPARENT),
    };
    let (fill,border)=match (role,state){
        (FieldAction|Button,VisualState::Disabled)=>(DISABLED_ACTION_BG,DISABLED_ACTION_BORDER),
        (NavigationSegment,VisualState::Disabled)=>(RECEPTION_RECIPIENT_NAV_DISABLED_BG,Color32::TRANSPARENT),
        (WorkflowCard,VisualState::Selected)=>(WORKFLOW_SELECTED_BG,WORKFLOW_CARD_BORDER),
        _=>(fill,border),
    };
    SurfacePalette{fill,border}
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum BorderRole{Standard,Divider,Focus}
pub(crate) const fn border_ink(role:BorderRole)->Color32{
    match role{BorderRole::Standard=>BORDER,BorderRole::Divider=>DIVIDER,BorderRole::Focus=>SELECTION}
}

/// Opaque white is the identity tint, not a UI foreground color.
pub(crate) const fn image_tint()->Color32{Color32::WHITE}
pub(crate) fn shadow_ink(alpha:u8)->Color32{Color32::from_black_alpha(alpha)}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum IconRole{Secondary,Interactive,Navigation,SortIndicator}
pub(crate) const fn icon_ink(role:IconRole,state:VisualState)->Color32{
    match (role,state){
        (IconRole::Navigation,VisualState::Disabled)=>RECEPTION_RECIPIENT_NAV_DISABLED_ICON,
        (_,VisualState::Disabled)=>text_ink(TextRole::Label,TextState::Disabled),
        (IconRole::SortIndicator,VisualState::Selected)=>SELECTION,
        (IconRole::SortIndicator,_)=>text_ink(TextRole::Metadata,TextState::Normal),
        (IconRole::Interactive|IconRole::Navigation,VisualState::Hovered)=>text_ink(TextRole::Value,TextState::Normal),
        _=>text_ink(TextRole::Label,TextState::Normal),
    }
}

pub(crate) fn role_surface_frame(role:SurfaceRole,state:VisualState,radius:u8)->Frame{
    let palette=surface_palette(role,state);
    bordered_surface(palette.fill,palette.border,radius)
}

pub(crate) fn button_visual(
    role:ButtonRole,state:VisualState,review_white:bool
)->(f32,Color32,Color32,Stroke){
    let enabled=state!=VisualState::Disabled;
    let (height, fill, color, stroke): (f32, Color32, Color32, Stroke) = match role {
        ButtonRole::Primary => (ACTION_PRIMARY_HEIGHT, PRIMARY_ACTION_FILL, BRIGHT_TEXT, Stroke::NONE),
        ButtonRole::Standard => (ACTION_STANDARD_HEIGHT, RAISED, TEXT, Stroke::new(FRAME_STROKE_WIDTH, BORDER)),
        ButtonRole::TableSecondary => (
            ACTION_STANDARD_HEIGHT,
            PANEL,
            SECONDARY,
            Stroke::new(FRAME_STROKE_WIDTH, DIVIDER),
        ),
        ButtonRole::Emphasized => (ACTION_EMPHASIZED_HEIGHT, BRIGHT, BRIGHT_TEXT, Stroke::NONE),
        ButtonRole::Quiet => (ACTION_QUIET_HEIGHT, PANEL, SECONDARY, Stroke::new(FRAME_STROKE_WIDTH,DIVIDER)),
        #[cfg(test)]
        ButtonRole::Ghost => (ACTION_PRIMARY_HEIGHT, Color32::TRANSPARENT, if enabled {SECONDARY} else {DISABLED_ACTION_TEXT}, Stroke::NONE),
        ButtonRole::Danger => (ACTION_STANDARD_HEIGHT, DANGER_ACTION_BG, ERROR_TEXT, Stroke::new(FRAME_STROKE_WIDTH,DANGER_ACTION_BORDER)),
        ButtonRole::Navigation => (NAV_HEIGHT, Color32::TRANSPARENT, SECONDARY, Stroke::NONE),
        ButtonRole::ActiveNavigation => (NAV_HEIGHT, ACTIVE_NAVIGATION_BG, TEXT, Stroke::NONE),
        ButtonRole::ManagementTab => (MANAGEMENT_TAB_HEIGHT, Color32::TRANSPARENT, SECONDARY, Stroke::NONE),
        ButtonRole::ActiveManagementTab => (MANAGEMENT_TAB_HEIGHT, RAISED, TEXT, Stroke::new(FRAME_STROKE_WIDTH,BORDER)),
        ButtonRole::KakaoControl => (KAKAO_CONTROL_HEIGHT, Color32::TRANSPARENT, SECONDARY, Stroke::NONE),
        ButtonRole::ActiveKakaoControl => (KAKAO_CONTROL_HEIGHT, RAISED, TEXT, Stroke::new(FRAME_STROKE_WIDTH,BORDER)),
    };
    // Standard management actions have an explicit disabled appearance.
    // egui's generic disabled tint alone was too close to the enabled state.
    // Callers resolve the parent scope before selecting the visual state.
    let (fill,color,stroke)=if review_white {
        if enabled {
            (BRIGHT, BRIGHT_TEXT, stroke)
        } else {
            (DISABLED_ACTION_BG, DISABLED_ACTION_TEXT, stroke)
        }
    }else if !enabled
        && matches!(role,
            ButtonRole::Primary | ButtonRole::Standard | ButtonRole::TableSecondary | ButtonRole::Emphasized | ButtonRole::Quiet | ButtonRole::Danger
        )
    {
        (DISABLED_ACTION_BG,DISABLED_ACTION_TEXT,
         Stroke::new(FRAME_STROKE_WIDTH,DISABLED_ACTION_BORDER))
    }else{(fill,color,stroke)};
    (height,fill,color,stroke)
}

pub(crate) fn field_visual(state:VisualState,enabled_ink:Color32)->(Color32,Color32,Stroke){
    let p=surface_palette(SurfaceRole::FieldAction,state);
    if state==VisualState::Disabled{
        (p.fill,text_ink(TextRole::Value,TextState::Disabled),Stroke::new(FRAME_STROKE_WIDTH,p.border))
    }else{
        (p.fill,enabled_ink,Stroke::new(SOFT_DIVIDER_WIDTH,p.border))
    }
}

#[cfg(test)]
mod shared_visual_role_contract_tests{
    use super::*;
    #[test]
    fn checkbox_and_scroll_rail_roles_preserve_existing_visual_contract(){
        let selected=checkbox_palette(true,VisualState::Selected).unwrap();
        let hovered=checkbox_palette(true,VisualState::Hovered).unwrap();
        assert_eq!(selected.fill,CHECKBOX_CHECKED_BG);
        assert_eq!(selected.outline.color,BORDER);
        assert_eq!(selected.fill,RAISED);
        assert_eq!(selected.mark.color,BRIGHT);
        assert_eq!(hovered.outline.color,SELECTION);
        assert_eq!(selected.fill,hovered.fill);
        assert_eq!(selected.mark,hovered.mark);
        assert_eq!(selected.mark.color,CHECKBOX_CHECKED_MARK);
        assert!(checkbox_palette(false,VisualState::Normal).is_none());
        assert!(checkbox_palette(true,VisualState::Disabled).is_none());
        let standard=scroll_rail_geometry(ScrollRailRole::Standard);
        let table=scroll_rail_geometry(ScrollRailRole::Table);
        assert_eq!(standard.width,10.0);
        assert_eq!(standard.minimum_handle,44.0);
        assert_eq!((standard.inner_margin,standard.outer_margin),(SPACE_4,SPACE_4));
        assert_eq!(table.width,standard.width);
        assert_eq!(table.minimum_handle,standard.minimum_handle);
        assert_eq!((table.inner_margin,table.outer_margin),(0.0,0.0));
    }

    #[test]
    fn base_surfaces_preserve_approved_colors_and_borders(){
        for (role,fill,border) in [
            (SurfaceRole::Workspace,CANVAS,Color32::TRANSPARENT),
            (SurfaceRole::Section,PANEL,CARD_BORDER),
            (SurfaceRole::Summary,PANEL,BORDER),
            (SurfaceRole::Input,INPUT,BORDER),
            (SurfaceRole::Inset,INPUT,DIVIDER),
            (SurfaceRole::Modal,PANEL,BORDER),
            (SurfaceRole::ModalSection,MODAL_SECTION_BG,DIVIDER),
            (SurfaceRole::AlternateRow,TABLE_ROW_ALT_BG,Color32::TRANSPARENT),
            (SurfaceRole::TableHeader,RAISED,BORDER),
            (SurfaceRole::RaisedInset,RAISED,DIVIDER),
            (SurfaceRole::Notice,FLOATING_NOTICE_BG,BORDER),
        ]{
            assert_eq!(surface_palette(role,VisualState::Normal),SurfacePalette{fill,border});
            assert_eq!(role_surface_frame(role,VisualState::Normal,CONTROL_RADIUS).fill,fill);
        }
        assert_eq!(surface_palette(SurfaceRole::FieldAction,VisualState::Disabled),
            SurfacePalette{fill:DISABLED_ACTION_BG,border:DISABLED_ACTION_BORDER});
    }
    #[test]
    fn button_icon_and_field_states_preserve_shared_palette(){
        for role in [ButtonRole::Primary,ButtonRole::Standard,ButtonRole::TableSecondary,
            ButtonRole::Emphasized,ButtonRole::Quiet,ButtonRole::Danger]{
            let (height,_,_,_)=button_visual(role,VisualState::Normal,false);
            let (disabled,fill,ink,stroke)=button_visual(role,VisualState::Disabled,false);
            assert_eq!(height,disabled);
            assert_eq!((fill,ink,stroke.color),(DISABLED_ACTION_BG,DISABLED_ACTION_TEXT,DISABLED_ACTION_BORDER));
        }
        assert_eq!(icon_ink(IconRole::SortIndicator,VisualState::Selected),SELECTION);
        assert_eq!(icon_ink(IconRole::SortIndicator,VisualState::Normal),MUTED);
        assert_eq!(icon_ink(IconRole::Interactive,VisualState::Hovered),TEXT);
        assert_eq!(icon_ink(IconRole::Interactive,VisualState::Normal),SECONDARY);
        assert_eq!(icon_ink(IconRole::Navigation,VisualState::Disabled),RECEPTION_RECIPIENT_NAV_DISABLED_ICON);
        assert_eq!(field_visual(VisualState::Normal,TEXT),(INPUT,TEXT,Stroke::new(SOFT_DIVIDER_WIDTH,BORDER)));
        assert_eq!(field_visual(VisualState::Disabled,TEXT),(DISABLED_ACTION_BG,DISABLED_ACTION_TEXT,Stroke::new(FRAME_STROKE_WIDTH,DISABLED_ACTION_BORDER)));
    }
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
    style.visuals.panel_fill = surface_palette(SurfaceRole::Workspace,VisualState::Normal).fill;
    style.visuals.window_fill = surface_palette(SurfaceRole::Panel,VisualState::Normal).fill;
    style.visuals.faint_bg_color = surface_palette(SurfaceRole::Button,VisualState::Normal).fill;
    style.visuals.extreme_bg_color = surface_palette(SurfaceRole::Input,VisualState::Normal).fill;
    style.visuals.override_text_color = Some(text_ink(TextRole::Value,TextState::Normal));
    style.visuals.widgets.noninteractive.bg_fill = surface_palette(SurfaceRole::Panel,VisualState::Normal).fill;
    style.visuals.widgets.inactive.bg_fill = surface_palette(SurfaceRole::Button,VisualState::Normal).fill;
    // Native egui controls, selectables, toggles and pop-up options inherit
    // the same clearly visible hover feedback as our authored buttons.
    // Keep hover blue distinct from committed orange order selection.
    crate::highlight_native::native_widgets(&mut style.visuals,crate::highlight_native::NativePalette{
        hover_fill:BUTTON_HOVER_BG,
        hover_outline:Stroke::new(BUTTON_HOVER_BORDER_WIDTH,SELECTION),
        hover_ink:text_ink(TextRole::Value,TextState::Normal),
        active_fill:surface_palette(SurfaceRole::Input,VisualState::Normal).fill,
        focus_outline:Stroke::new(BUTTON_HOVER_BORDER_WIDTH,SELECTION),
        selection_fill:SELECTION.linear_multiply(0.3),
        selection_outline:Stroke::new(SELECTION_STROKE_WIDTH,SELECTION),
    });
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(FRAME_STROKE_WIDTH, BORDER);
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(FRAME_STROKE_WIDTH, BORDER);
    style.spacing.item_spacing = Vec2::new(SPACING, SPACING);
    // Per-role horizontal padding is applied by action() below.
    style.spacing.button_padding = Vec2::new(SPACE_12, SPACE_8);
    ctx.set_style(style);
}

/// Screen-specific roles retain their approved appearance, but frame
/// construction and padding belong to the shared theme.
pub fn navigation_header_frame()->Frame{
    Frame::new().fill(surface_palette(SurfaceRole::Header,VisualState::Normal).fill)
        .inner_margin(egui::Margin::symmetric(HEADER_PADDING_X,0))
}
pub fn management_page_host_frame()->Frame{
    Frame::new().inner_margin(0)
}
/// Transparent heading inset, not a second card surface.
pub fn management_top_content_frame()->Frame{
    Frame::new().inner_margin(egui::Margin::symmetric(SECTION_PADDING as i8,0))
}
/// Base visual surface shared by cards and modal sections. Role wrappers
/// retain their approved radius, padding, fill and border color.
pub(crate) fn bordered_surface(fill:Color32,border:Color32,radius:u8)->Frame{
    Frame::new().fill(fill)
        .stroke(surface_stroke(border))
        .corner_radius(CornerRadius::same(radius))
}

fn surface_stroke(border:Color32)->Stroke{
    Stroke::new(FRAME_STROKE_WIDTH,border)
}

/// Fixed-rectangle surface: keep layout and hit geometry with the caller,
/// while the common surface owns the 1pt inside stroke.
pub(crate) fn paint_bordered_surface(
    painter:&egui::Painter,rect:egui::Rect,radius:impl Into<CornerRadius>,
    fill:Color32,border:Color32
){
    painter.rect(rect,radius,fill,surface_stroke(border),egui::StrokeKind::Inside);
}

/// Finish a surface after painting its image or composite content.
pub(crate) fn paint_surface_outline(
    painter:&egui::Painter,rect:egui::Rect,radius:impl Into<CornerRadius>,border:Color32
){
    painter.rect_stroke(rect,radius,surface_stroke(border),egui::StrokeKind::Inside);
}

pub const MANAGEMENT_MASTER_TABLE_BOTTOM_PADDING:f32=SECTION_PADDING;

/// Management tables use a permanent, non-overlay scrollbar track.
/// Solid scrollbars remain visible while a list has overflow and never
/// collapse into a faint 2pt floating line when the pointer leaves.
/// Reserve the width in layout so row buttons do not sit underneath it.
/// The native rail begins at the first row's exact top edge: a 3pt top
/// inset left a visible break beneath the full-width fixed-header border.
/// Keep this zero for all management tables sharing that header role.
#[cfg(test)]
pub const MANAGEMENT_TABLE_SCROLL_TRACK_TOP_INSET:f32=0.0;
/// Table rails end at the body boundary. Card padding is independent.
#[cfg(test)]
pub const MANAGEMENT_TABLE_SCROLL_TRACK_BOTTOM_INSET:f32=0.0;
#[cfg(test)]
pub const MANAGEMENT_TABLE_SCROLL_RAIL_EXTENSION:f32=0.0;

/// The same outer gutter for Reception and every management page:
/// 6pt CANVAS above, right and below; the KakaoTalk reservation stays
/// flush with the left edge. The content/splitter layout is measured
/// INSIDE this inset, so existing saved column ratios and native HWND
/// placement remain based on the actual available workspace.
pub fn workspace_shell_frame() -> Frame {
    Frame::new()
        .fill(surface_palette(SurfaceRole::Workspace,VisualState::Normal).fill)
        .stroke(Stroke::NONE)
        .inner_margin(egui::Margin {
            left: 0,
            right: PANEL_GAP as i8,
            top: PANEL_GAP as i8,
            bottom: PANEL_GAP as i8,
        })
}

/// The reserved KakaoTalk area is also workspace, whether connected or
/// empty. Preserve its full slot geometry for the native HWND docking.
pub fn kakao_slot_frame() -> Frame {
    Frame::new()
        // One palette level above the canvas (#101010 -> #181818).
        // Only the Kakao reservation changes; native HWND geometry is intact.
        .fill(surface_palette(SurfaceRole::Panel,VisualState::Normal).fill)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(0))
        .inner_margin(0)
}

/// Shared status notice palette. Color belongs to the entire card; no
/// shadow, glow, animation or hover-induced geometry is applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewTone {
    Action,
    Info,
    Error,
    Complete,
}

impl ReviewTone {
    pub const fn palette(self) -> (Color32, Color32, Color32, Color32) {
        // Background, border, primary text, secondary text.
        match self {
            Self::Action => (
                ORANGE_EMPHASIS_BG, REQUIRED_REVIEW_BORDER,
                REQUIRED_REVIEW_TEXT, Color32::from_rgb(217,188,152),
            ),
            Self::Info => (
                Color32::from_rgb(25, 45, 64), Color32::from_rgb(88, 152, 218),
                Color32::from_rgb(160, 205, 255), Color32::from_rgb(144, 172, 199),
            ),
            Self::Error => (
                ERROR_BG, ERROR_BORDER,
                ERROR_TEXT, Color32::from_rgb(223, 165, 165),
            ),
            Self::Complete => (
                Color32::from_rgb(29, 53, 38), Color32::from_rgb(89, 167, 117),
                Color32::from_rgb(155, 221, 176), Color32::from_rgb(153, 197, 167),
            ),
        }
    }

    pub const fn text_color(self) -> Color32 { self.palette().2 }
    pub fn detail_color(self) -> Color32 { self.palette().3 }
}

#[cfg(test)]
#[test]
fn review_action_and_one_touch_share_orange_palette(){
    let (background,border,text,detail)=ReviewTone::Action.palette();
    assert_eq!((background,border,text),(ONE_TOUCH_BG,ONE_TOUCH_BORDER,ONE_TOUCH_TEXT));
    assert_eq!(detail,Color32::from_rgb(217,188,152));
    assert_ne!(detail,text);
    assert_ne!(background,PANEL);
}

/// Shared solid divider surface for horizontal and vertical rules.
pub(crate) fn paint_soft_rule(painter:&egui::Painter,rect:egui::Rect){
    painter.rect_filled(rect,0.0,DIVIDER);
}

/// One soft horizontal rule. The reserved rectangle and painted surface
/// coincide: no centered stroke protrudes into the preceding row.
pub fn soft_rule(ui:&mut Ui)->egui::Rect{
    let (rect,_)=ui.shared_allocate_exact_size(
        Vec2::new(ui.available_width(),SOFT_DIVIDER_WIDTH),egui::Sense::hover()
    );
    paint_soft_rule(ui.painter(),rect);
    rect
}

#[cfg(test)]
mod management_primary_card_geometry_tests {
    use super::*;

    #[test]
    fn approved_master_table_specs_preserve_columns_and_spare_space() {
        let freight = FREIGHT_ASSIGNMENT_MASTER_TABLE;
        let history = ORDER_HISTORY_MASTER_TABLE;
        assert_eq!(freight.min_width,942.0);
        assert_eq!(history.min_width,1242.0);
        assert_eq!(FREIGHT_RATE_TABLE_MIN_WIDTH,1265.0);
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
        assert!((management_primary_card_top_offset(1200.0)-208.0).abs()<0.01);
    }

    #[test]
    fn narrower_workspaces_reserve_space_for_wrapped_freight_kpis() {
        assert!((management_primary_card_top_offset(650.0)-296.0).abs()<0.01);
        assert!((management_primary_card_top_offset(250.0)-472.0).abs()<0.01);
        // A 950pt pane fits four cards before the 12pt + 12pt page inset,
        // but only three inside it; reserve the wrapped second row.
        assert!((management_primary_card_top_offset(950.0)-296.0).abs()<0.01);
        assert!((management_primary_card_top_offset(968.0)-208.0).abs()<0.01);
    }
}

