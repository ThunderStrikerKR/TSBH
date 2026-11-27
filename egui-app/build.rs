// Reuse the already-shipped Tauri Windows .ico in the native egui EXE.
// This is a build resource only; do not copy or regenerate brand artwork.
fn main() {
    println!("cargo:rerun-if-changed=../src-tauri/icons/icon.ico");
    // React __BANDO_BUILD_ID__ uses 'git rev-parse --short=7 HEAD'.
    // Preserve that exact provenance for the native Settings/System tab.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    let hash = std::env::var("GITHUB_SHA").ok()
        .filter(|sha|sha.len()>=7)
        .map(|sha|sha[..7].to_owned())
        .or_else(||{
            std::process::Command::new("git")
                .args(["rev-parse","--short=7","HEAD"])
                .output().ok().filter(|result|result.status.success())
                .and_then(|result|String::from_utf8(result.stdout).ok())
                .map(|sha|sha.trim().to_owned())
                .filter(|sha|!sha.is_empty())
        }).unwrap_or_else(||"확인 불가".to_owned());
    println!("cargo:rustc-env=BANDO_BUILD_ID={hash}");
    #[cfg(target_os = "windows")]
    {
        let mut resource = winres::WindowsResource::new();
        resource.set_icon("../src-tauri/icons/icon.ico");
        resource.set("ProductName", "BANDO HUB");
        resource.set("ProductVersion", env!("CARGO_PKG_VERSION"));
        resource.set("FileVersion", env!("CARGO_PKG_VERSION"));
        resource.compile().expect("기존 반도허브 Windows 아이콘 삽입 실패");
    }
}
