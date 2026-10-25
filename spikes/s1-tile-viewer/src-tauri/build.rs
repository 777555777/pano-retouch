fn main() {
    // S1_XCHECK=1: type-check the Windows code from macOS without the Windows resource compiler.
    if std::env::var_os("S1_XCHECK").is_some() {
        println!("cargo:rustc-check-cfg=cfg(desktop)");
        println!("cargo:rustc-check-cfg=cfg(mobile)");
        println!("cargo:rustc-cfg=desktop");
        println!("cargo:rustc-check-cfg=cfg(s1_xcheck)");
        println!("cargo:rustc-cfg=s1_xcheck");
        return;
    }
    println!("cargo:rustc-check-cfg=cfg(s1_xcheck)");
    tauri_build::build()
}
