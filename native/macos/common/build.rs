fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    // XPC's blocks-based connection handler is the only producer transport C glue.
    cc::Build::new()
        .file("../application-capture/src/TNApplicationSourceTransport.c")
        .include("../hal-driver/include")
        .flag("-std=c17")
        .flag("-fblocks")
        .flag("-mmacosx-version-min=14.2")
        .warnings_into_errors(true)
        .compile("tonenari_source_xpc");
    println!("cargo:rerun-if-changed=../application-capture/src/TNApplicationSourceTransport.c");
    println!("cargo:rerun-if-changed=../hal-driver/include/TNHALControlProtocol.h");
}
