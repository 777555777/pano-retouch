fn main() {
    // S3: the prebuilt ONNX Runtime uses `@available` checks, which need clang's runtime
    // (`___isPlatformVersionAtLeast`); rustc links with -nodefaultlibs, so add it explicitly.
    if std::env::var_os("CARGO_FEATURE_AI").is_some() && std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        if let Ok(out) = std::process::Command::new("clang").arg("--print-resource-dir").output() {
            let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
            println!("cargo:rustc-link-search=native={dir}/lib/darwin");
            println!("cargo:rustc-link-lib=static=clang_rt.osx");
        }
    }
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
