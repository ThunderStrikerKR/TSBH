//! Geometry shared by all resizable egui workspaces.
//! The four columns preserve the React information architecture while
//! avoiding reflow when a neighboring card changes its contents.

pub const PANEL_COUNT: usize = 4;
pub const SPLITTER: f32 = crate::ui_tokens::WORKSPACE_SPLITTER_WIDTH;
// Original 1920x1080 React reference: 390/531/529/418 physical pixels.
// The current default below deliberately transfers width from the source
// pane to the details pane; the reference remains useful for minimums.
// The previous last-pane 420pt minimum forced it to ~25.4% on this screen
// despite a reference width of only 22.4%. 350pt clears that clamp while
// retaining a readable minimum on smaller windows.
// React HubShell's Kakao minimum is 392 CSS px at 100% UI scale.
// Do not reserve only 200pt then ask the employee to resize Kakao manually.
pub const MIN_WIDTHS: [f32; PANEL_COUNT] = [392.0, 250.0, 250.0, 350.0];
// Reception's three work panes follow Kakao: source, details, orders.
// Widen details by 15% relative to its previous 28.3 share, funded solely
// from source (4.245 percentage points). Kakao and orders stay unchanged.
pub const DEFAULT_RATIOS: [f32; PANEL_COUNT] = [20.9, 24.155, 32.545, 22.4];
const PREVIOUS_DEFAULT_RATIOS: [f32; PANEL_COUNT] = [20.9, 28.4, 28.3, 22.4];
const LEGACY_DEFAULT_RATIOS: [f32; PANEL_COUNT] = [20.0, 29.0, 29.0, 22.0];
// The middle column owns a fixed recipient card and a fixed order total.
// The two remaining cards share the free height equally, independent of
// persisted source/message or legacy middle-column splitter ratios.
pub const SOURCE_MIN: f32 = 275.0;
pub const MESSAGE_MIN: f32 = 160.0;
pub const VERTICAL_DEFAULT: f32 = 0.5;

#[derive(Clone, Debug)]
pub struct LayoutState {
    pub ratios: [f32; PANEL_COUNT],
    pub source_ratio: f32,
}

impl Default for LayoutState {
    fn default() -> Self {
        Self {
            ratios: DEFAULT_RATIOS,
            source_ratio: VERTICAL_DEFAULT,
        }
    }
}

impl LayoutState {
    pub fn minimum_width() -> f32 {
        MIN_WIDTHS.iter().sum::<f32>() + SPLITTER * (PANEL_COUNT as f32 - 1.0)
    }

    fn preview_layout_path() -> Option<std::path::PathBuf> {
        // Use a separate location. NEVER read or overwrite the production
        // Tauri/SQLite database or the React application's settings.
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return Some(std::path::PathBuf::from(local)
                .join("BANDO-HUB-egui-preview").join("layout.txt"));
        }
        std::env::var_os("HOME").map(|home| {
            std::path::PathBuf::from(home)
                .join(".config").join("bando-hub-egui-preview").join("layout.txt")
        })
    }

    fn scoped_layout_path() -> Option<std::path::PathBuf> {
        crate::window_state::ui_state_path("layout.txt")
    }

    pub fn load() -> Self {
        // The former shared egui preview file is a read-only migration seed.
        // Once a mode saves, it never reads or overwrites the other mode.
        let raw = Self::scoped_layout_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .or_else(|| Self::preview_layout_path()
                .and_then(|p| std::fs::read_to_string(p).ok()));
        Self::from_string(raw)
    }

    pub fn save(&self) -> std::io::Result<()> {
        if let Some(path) = Self::scoped_layout_path() {
            crate::window_state::write_atomic(&path,self.as_string().as_bytes())?;
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn from_string(raw: Option<String>) -> Self {
        let Some(raw) = raw else { return Self::default(); };
        let Ok(parts) = raw.split(',').map(str::parse::<f32>).collect::<Result<Vec<_>,_>>() else{
            return Self::default();
        };
        if (parts.len() != PANEL_COUNT + 1 && parts.len() != PANEL_COUNT + 3) ||
            parts[..PANEL_COUNT + 1].iter().any(|n| !n.is_finite() || *n <= 0.0) {
            return Self::default();
        }
        let ratios: [f32; PANEL_COUNT] = match parts[..PANEL_COUNT].try_into() {
            Ok(ratios) => ratios,
            Err(_) => return Self::default(),
        };
        let ratio_total: f32 = ratios.iter().sum();
        if !ratio_total.is_finite() || ratio_total <= 0.0 {
            return Self::default();
        }
        // Migrate saved unmodified defaults, including the preceding preset.
        // Keep manually resized column widths and the source/message splitter.
        let unmodified_legacy_preset = [LEGACY_DEFAULT_RATIOS, PREVIOUS_DEFAULT_RATIOS]
            .iter()
            .any(|preset| ratios.iter().zip(preset.iter())
                .all(|(value, legacy)| (*value - *legacy).abs() < 0.00001));
        Self {
            ratios: if unmodified_legacy_preset { DEFAULT_RATIOS } else { ratios },
            source_ratio: parts[PANEL_COUNT].clamp(0.05, 0.95),
            // Legacy seven-value layouts are accepted, but their last two
            // middle-column splitter ratios no longer control geometry.
        }
    }

    pub fn as_string(&self) -> String {
        format!(
            "{:.5},{:.5},{:.5},{:.5},{:.5}",
            self.ratios[0], self.ratios[1],
            self.ratios[2], self.ratios[3], self.source_ratio
        )
    }

    /// Return columns that fill the available area, respect independent
    /// minimum widths and preserve the relative weights of unclamped panes.
    pub fn widths(&self, full_width: f32) -> [f32; PANEL_COUNT] {
        let width = (full_width - SPLITTER * (PANEL_COUNT as f32 - 1.0))
            .max(MIN_WIDTHS.iter().sum());
        let mut values = [0.0; PANEL_COUNT];
        let mut locked = [false; PANEL_COUNT];
        let mut remaining = width;
        loop {
            let unlocked: Vec<usize> = (0..PANEL_COUNT).filter(|&i| !locked[i]).collect();
            if unlocked.is_empty() {
                // Defensive rounding path: distribute any remainder without
                // violating minimum widths or leaving an unused right gutter.
                values[PANEL_COUNT - 1] += remaining.max(0.0);
                break;
            }
            let sum: f32 = unlocked.iter().map(|&i| self.ratios[i]).sum();
            if sum <= 0.0 {
                let per_column = remaining / unlocked.len() as f32;
                for i in unlocked { values[i] = per_column; }
                break;
            }

            // Decide all clamped columns from ONE stable remaining budget.
            // Mutating 'remaining' inside this loop made neighboring panels
            // clamp prematurely and could leave a 12-point empty gutter.
            let to_lock: Vec<usize> = unlocked.iter().copied()
                .filter(|&i| remaining * self.ratios[i] / sum < MIN_WIDTHS[i])
                .collect();
            if to_lock.is_empty() {
                for i in unlocked {
                    values[i] = remaining * self.ratios[i] / sum;
                }
                break;
            }
            for i in to_lock {
                values[i] = MIN_WIDTHS[i];
                locked[i] = true;
                remaining -= MIN_WIDTHS[i];
            }
        }
        values
    }

    pub fn resize_pair(&mut self, widths: [f32; PANEL_COUNT], index: usize, delta: f32) {
        if index >= PANEL_COUNT - 1 || !delta.is_finite() { return; }
        let pair = widths[index] + widths[index + 1];
        if pair < MIN_WIDTHS[index] + MIN_WIDTHS[index + 1] { return; }
        let target = (widths[index] + delta)
            .clamp(MIN_WIDTHS[index], pair - MIN_WIDTHS[index + 1]);
        let mut widths = widths;
        widths[index] = target;
        widths[index + 1] = pair - target;
        self.ratios = widths;
    }

    /// Restore only the selected vertical column divider, without resetting others.
    pub fn reset_pair(&mut self, widths: [f32; PANEL_COUNT], index: usize) {
        if index >= PANEL_COUNT - 1 { return; }
        let pair = widths[index] + widths[index + 1];
        let default_pair = DEFAULT_RATIOS[index] + DEFAULT_RATIOS[index + 1];
        let target = pair * DEFAULT_RATIOS[index] / default_pair;
        self.resize_pair(widths, index, target - widths[index]);
    }

    pub fn vertical_heights(&self, available: f32) -> (f32, f32) {
        let space = (available - SPLITTER).max(SOURCE_MIN + MESSAGE_MIN);
        let first = (space * self.source_ratio)
            .clamp(SOURCE_MIN, space - MESSAGE_MIN);
        (first, space - first)
    }

    /// [review, recipient, items + summary gap + summary]. Reserve the
    /// two section gutters and the summary first; split the rest 50:50.
    /// In a short window reduce the recipient viewport only as necessary
    /// to protect the two variable card headers. Its body can scroll.
    pub fn detail_heights(&self, available: f32) -> [f32; 3] {
        use crate::theme;
        let space = (available - 2.0 * SPLITTER).max(0.0);
        let summary = (theme::RECEPTION_ORDER_TOTAL_SECTION_HEIGHT
            + theme::RECEPTION_ORDER_TOTAL_SECTION_GAP).min(space);
        let remaining = (space - summary).max(0.0);
        let recipient = (remaining - 2.0 * theme::RECEPTION_SECTION_CHROME_HEIGHT)
            .max(0.0).min(theme::RECEPTION_RECIPIENT_SECTION_HEIGHT);
        let variable = (remaining - recipient) * 0.5;
        [variable, recipient, space - variable - recipient]
    }

    pub fn resize_vertical(&mut self, current_first: f32, delta: f32, available: f32) {
        let space = (available - SPLITTER).max(SOURCE_MIN + MESSAGE_MIN);
        let first = (current_first + delta).clamp(SOURCE_MIN, space - MESSAGE_MIN);
        self.source_ratio = first / space;
    }

    pub fn reset_vertical(&mut self) {
        self.source_ratio = VERTICAL_DEFAULT;
    }
}


// Small, deterministic geometry-contract checks only. These are intentionally
// independent of order contents, screenshots, Tauri, CI and production data.
#[cfg(test)]
mod geometry_contract {
    use super::*;

    #[test]
    fn reference_1920p_columns_preserve_ratios_after_min_width_clamping() {
        // At unscaled 1920px all default shares fit; at the scaled 1637pt
        // reference the 392pt Kakao and 350pt order minimums take priority.
        for width in [1637.0, 1908.0] {
            let columns = LayoutState::default().widths(width);
            let total: f32 = columns.iter().sum();
            let target = width - SPLITTER * (PANEL_COUNT - 1) as f32;
            assert!((total - target).abs() < 0.01);
            let expected = if width == 1637.0 {
                let remaining = target - MIN_WIDTHS[0] - MIN_WIDTHS[3];
                let weight = DEFAULT_RATIOS[1] + DEFAULT_RATIOS[2];
                [MIN_WIDTHS[0], remaining * DEFAULT_RATIOS[1] / weight,
                    remaining * DEFAULT_RATIOS[2] / weight, MIN_WIDTHS[3]]
            } else {
                DEFAULT_RATIOS.map(|ratio| target * ratio / 100.0)
            };
            for i in 0..PANEL_COUNT {
                assert!((columns[i] - expected[i]).abs() < 0.02,
                    "width={width}, column {i}: actual={}, expected={}", columns[i], expected[i]);
                assert!(columns[i] >= MIN_WIDTHS[i]);
            }
        }
    }

    #[test]
    fn custom_saved_split_ratios_roundtrip_without_geometry_drift() {
        let state = LayoutState {
            ratios: [420.0, 410.0, 530.0, 390.0],
            source_ratio: 0.63,
        };
        let restored = LayoutState::from_string(Some(state.as_string()));
        for width in [1260.0, 1637.0, 1908.0] {
            assert_eq!(state.widths(width), restored.widths(width));
        }
        for height in [720.0, 900.0, 1080.0] {
            assert_eq!(state.vertical_heights(height), restored.vertical_heights(height));
            assert_eq!(state.detail_heights(height), restored.detail_heights(height));
        }
    }

    #[test]
    fn legacy_preset_migrates_but_manually_saved_splits_survive() {
        let legacy = LayoutState::from_string(Some(
            "20,29,29,22,0.65".to_owned()
        ));
        assert_eq!(legacy.ratios, DEFAULT_RATIOS);
        assert!((legacy.source_ratio - 0.65).abs() < 0.00001);

        let custom = LayoutState::from_string(Some(
            "350,440,500,380,0.65".to_owned()
        ));
        assert_eq!(custom.ratios, [350.0, 440.0, 500.0, 380.0]);
        assert!((custom.source_ratio - 0.65).abs() < 0.00001);
    }

    #[test]
    fn middle_column_splits_remaining_space_equally_with_fixed_recipient_and_summary() {
        use crate::theme;
        assert_eq!(theme::RECEPTION_RECIPIENT_SECTION_HEIGHT,235.0);
        assert_eq!(theme::RECEPTION_ORDER_TOTAL_SECTION_HEIGHT,123.0);
        let reserved=theme::RECEPTION_ORDER_TOTAL_SECTION_HEIGHT
            +theme::RECEPTION_ORDER_TOTAL_SECTION_GAP;
        for height in [600.0,720.0,900.0,1000.0,1200.0,1400.0] {
            let [review,recipient,bottom]=LayoutState::default().detail_heights(height);
            let items=bottom-reserved;
            assert!((recipient-235.0).abs()<0.02);
            assert!((review-items).abs()<0.02);
            assert!((review+recipient+bottom+2.0*SPLITTER-height).abs()<0.02);
        }
    }

    #[test]
    fn legacy_middle_ratios_are_ignored_but_other_splits_survive() {
        let legacy=LayoutState::from_string(Some("350,440,500,380,0.65,0.10,0.90".into()));
        assert_eq!(legacy.ratios,[350.0,440.0,500.0,380.0]);
        assert!((legacy.source_ratio-0.65).abs()<0.00001);
        assert_eq!(legacy.as_string().split(',').count(),PANEL_COUNT+1);
        for height in [500.0,700.0,1000.0,1200.0] {
            assert_eq!(legacy.detail_heights(height),LayoutState::default().detail_heights(height));
        }
    }

    #[test]
    fn dragging_source_divider_never_moves_middle_column_boundaries() {
        let mut layout=LayoutState::default();
        for height in [700.0,800.0,1000.0,1200.0] {
            let expected=layout.detail_heights(height);
            let original=layout.vertical_heights(height).0;
            for delta in [0.2,1.0,3.0,40.0,-55.0] {
                layout.resize_vertical(original,delta,height);
                assert_eq!(layout.detail_heights(height),expected);
            }
            layout.reset_vertical();
        }
    }

    #[test]
    fn compact_middle_column_keeps_equal_variable_cards_and_bounded_recipient() {
        use crate::theme;
        let reserved=theme::RECEPTION_ORDER_TOTAL_SECTION_HEIGHT
            +theme::RECEPTION_ORDER_TOTAL_SECTION_GAP;
        for height in [300.0,400.0,500.0,512.0,600.0] {
            let [review,recipient,bottom]=LayoutState::default().detail_heights(height);
            assert!(recipient>=0.0 && recipient<=theme::RECEPTION_RECIPIENT_SECTION_HEIGHT);
            assert!(review+0.02>=theme::RECEPTION_SECTION_CHROME_HEIGHT);
            assert!((review-(bottom-reserved)).abs()<0.02);
            assert!((review+recipient+bottom+2.0*SPLITTER-height).abs()<0.02);
        }
    }
}
