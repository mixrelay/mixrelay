fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    cc::Build::new()
        .file("../driver/TNHALControlBlock.c")
        .flag("-std=c17")
        .flag("-fblocks")
        .warnings_into_errors(true)
        .compile("tonenari_hal_control_block");
    println!("cargo:rerun-if-changed=../driver/TNHALControlBlock.c");
}
