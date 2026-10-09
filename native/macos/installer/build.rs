fn main() {
    println!("cargo:rerun-if-changed=src/removal/native.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("src/removal/native.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .flag("-Werror")
            .compile("mixrelay_removal_authorization");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=framework=Security");
    }
}
