//! Dependency-free shared spacing and font metrics.
pub const SPACE_2:f32=2.0;
pub const SPACE_4:f32=4.0;
pub const SPACE_6:f32=6.0;
pub const SPACE_8:f32=8.0;
pub const SPACE_12:f32=12.0;
pub const SPACE_16:f32=16.0;
pub const SPACE_24:f32=24.0;
/// Shared draggable workspace separator geometry, independent of persistence.
pub const WORKSPACE_SPLITTER_WIDTH:f32=SPACE_6;
pub const FONT_FIELD_LABEL: f32 = 13.0;
pub const FONT_FREIGHT_SUMMARY_NUMBER: f32 = 28.0;
pub const FONT_FREIGHT_SUMMARY_LABEL: f32 = 13.0;
pub const FONT_PAGE_HEADING: f32 = 20.0;

// Geometry and glyph layout use the same line boxes.
pub const FREIGHT_SUMMARY_NUMBER_LINE_HEIGHT:f32=FONT_FREIGHT_SUMMARY_NUMBER+6.0;
pub const FREIGHT_SUMMARY_LABEL_LINE_HEIGHT:f32=FONT_FREIGHT_SUMMARY_LABEL+5.0;

// Shared presentation-state lifetime; dependency-free so primitive UI
// components never depend on navigation widgets.
pub(crate) const NAVIGATION_SNAPSHOT_TTL:std::time::Duration=std::time::Duration::from_secs(30*60);
