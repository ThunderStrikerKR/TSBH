// Reuse the already-shipped Tauri Windows .ico in the native egui EXE.
// This is a build resource only; do not copy or regenerate brand artwork.
fn main() {
    println!("cargo:rerun-if-changed=../src-tauri/icons/icon.ico");
    // React __BANDO_BUILD_ID__ uses 'git rev-parse --short=7 HEAD'.
    // Preserve that exact provenance for the native Settings/System tab.
    // HEAD may only contain a symbolic ref, so its contents do not change on
    // a normal commit. Watch the resolved ref and packed refs as well.
    let git_output=|args:&[&str]|{
        std::process::Command::new("git").args(args).output().ok()
            .filter(|result|result.status.success())
            .and_then(|result|String::from_utf8(result.stdout).ok())
            .map(|value|value.trim().to_owned()).filter(|value|!value.is_empty())
    };
    let mut tracked=vec!["HEAD".to_owned(),"packed-refs".to_owned()];
    if let Some(reference)=git_output(&["symbolic-ref","-q","HEAD"]){tracked.push(reference);}
    for reference in tracked{
        if let Some(path)=git_output(&["rev-parse","--git-path",&reference]){
            // Missing loose refs are represented by packed-refs instead.
            if std::path::Path::new(&path).exists(){println!("cargo:rerun-if-changed={path}");}
            else if let Some(parent)=std::path::Path::new(&path).parent(){
                if parent.exists(){println!("cargo:rerun-if-changed={}",parent.display());}
            }
        }
    }
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    let hash=git_output(&["rev-parse","--short=7","HEAD"])
        .or_else(||std::env::var("GITHUB_SHA").ok()
            .filter(|sha|sha.len()>=7&&sha.is_ascii()&&sha.bytes().all(|b|b.is_ascii_hexdigit()))
            .map(|sha|sha[..7].to_owned()))
        .unwrap_or_else(||"확인 불가".to_owned());
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
