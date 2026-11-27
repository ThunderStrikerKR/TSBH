//! Launch-only execution profile. No database, UI renderer or persistence dependency.
pub fn test_launch()->bool{
    cfg!(windows) && cfg!(debug_assertions) &&
        std::env::var("BANDO_EGUI_DEVELOPER_TEST").ok().as_deref()==Some("1")
}
