//! Geometry shared by all resizable egui workspaces.
//! The four columns preserve the React information architecture while
//! avoiding reflow when a neighboring card changes its contents.

pub const PANEL_COUNT: usize = 4;
pub const SPLITTER: f32 = 6.0;
pub const MIN_WIDTHS: [f32; PANEL_COUNT] = [200.0, 250.0, 250.0, 420.0];
pub const DEFAULT_RATIOS: [f32; PANEL_COUNT] = [20.0, 29.0, 29.0, 22.0];
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
        Self { ratios: DEFAULT_RATIOS, source_ratio: VERTICAL_DEFAULT }
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

    pub fn load() -> Self {
        let path = Self::preview_layout_path();
        Self::from_string(path.and_then(|p| std::fs::read_to_string(p).ok()))
    }

    pub fn save(&self) -> std::io::Result<()> {
        if let Some(path) = Self::preview_layout_path() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, self.as_string())?;
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn from_string(raw: Option<String>) -> Self {
        let Some(raw) = raw else { return Self::default(); };
        let parts: Vec<f32> = raw.split(',').filter_map(|n| n.parse::<f32>().ok()).collect();
        if parts.len() != PANEL_COUNT + 1 ||
            parts.iter().any(|n| !n.is_finite() || *n <= 0.0) {
            return Self::default();
        }
        let ratios: [f32; PANEL_COUNT] = match parts[..PANEL_COUNT].try_into() {
            Ok(ratios) => ratios,
            Err(_) => return Self::default(),
        };
        let ratio_total: f32 = ratios.iter().sum();
        if ratio_total <= 0.0 {
            return Self::default();
        }
        Self { ratios, source_ratio: parts[PANEL_COUNT].clamp(0.05, 0.95) }
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
            let sum: f32 = (0..PANEL_COUNT).filter(|&i| !locked[i])
                .map(|i| self.ratios[i]).sum();
            if sum <= 0.0 { break; }
            let mut clamped = false;
            for i in 0..PANEL_COUNT {
                if !locked[i] && remaining * self.ratios[i] / sum < MIN_WIDTHS[i] {
                    values[i] = MIN_WIDTHS[i];
                    locked[i] = true;
                    remaining -= MIN_WIDTHS[i];
                    clamped = true;
                }
            }
            if !clamped {
                for i in 0..PANEL_COUNT {
                    if !locked[i] {
                        values[i] = remaining * self.ratios[i] / sum;
                    }
                }
                break;
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

    pub fn vertical_heights(&self, available: f32) -> (f32, f32) {
        let space = (available - SPLITTER).max(SOURCE_MIN + MESSAGE_MIN);
        let first = (space * self.source_ratio)
            .clamp(SOURCE_MIN, space - MESSAGE_MIN);
        (first, space - first)
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
