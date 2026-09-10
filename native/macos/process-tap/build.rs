fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let root = "../application-capture";
    cc::Build::new()
        .file(format!("{root}/src/TNProcessTap.m"))
        .include(format!("{root}/include"))
        .flag("-fobjc-arc")
        .flag("-fblocks")
        .flag("-mmacosx-version-min=14.2")
        .warnings_into_errors(true)
        .compile("tonenari_process_tap_objc");
    println!("cargo:rerun-if-changed={root}/src/TNProcessTap.m");
    println!("cargo:rerun-if-changed={root}/include");
    for framework in ["CoreAudio", "CoreFoundation", "Foundation", "AppKit"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
}
