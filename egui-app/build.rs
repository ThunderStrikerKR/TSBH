// Reuse the already-shipped Tauri Windows .ico in the native egui EXE.
// This is a build resource only; do not copy or regenerate brand artwork.
fn main() {
    println!("cargo:rerun-if-changed=../src-tauri/icons/icon.ico");
    #[cfg(target_os = "windows")]
    {
        let mut resource = winres::WindowsResource::new();
        resource.set_icon("../src-tauri/icons/icon.ico");
        resource.set("ProductName", "BANDO HUB");
        resource.compile().expect("기존 반도허브 Windows 아이콘 삽입 실패");
    }
}
