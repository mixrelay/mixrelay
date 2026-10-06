fn main() {
    println!("cargo:rustc-check-cfg=cfg(mixrelay_application_clock_fixture)");
    println!("cargo:rerun-if-env-changed=MIXRELAY_APPLICATION_CLOCK_FIXTURE");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        if is_driver_manager_build() {
            embed_driver_identity();
        } else {
            build_macos_application_capture();
        }
    }
    tauri_build::build();
}

fn is_driver_manager_build() -> bool {
    std::env::var("TAURI_CONFIG")
        .ok()
        .and_then(|config| serde_json::from_str::<serde_json::Value>(&config).ok())
        .is_some_and(|config| config["identifier"] == "io.mixrelay.driver.desktop")
}

fn embed_driver_identity() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("Cargo provides CARGO_MANIFEST_DIR to build scripts");
    let info_plist = std::path::Path::new(&manifest_dir).join("Info.driver.plist");
    println!(
        "cargo:rustc-link-arg=-Wl,-sectcreate,__TEXT,__info_plist,{}",
        info_plist.display()
    );
    println!("cargo:rerun-if-changed={}", info_plist.display());
}

fn build_macos_application_capture() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("Cargo provides CARGO_MANIFEST_DIR to build scripts");
    let info_plist = std::path::Path::new(&manifest_dir).join("Info.plist");
    let native_root = "../../../native/macos/application-capture";
    let engine_root = "../../../native/macos/engine-service";
    let hal_root = "../../../native/macos/hal-driver";
    let include = format!("{native_root}/include");
    let engine_include = format!("{engine_root}/include");
    let hal_include = format!("{hal_root}/include");

    let live_fixture = std::env::var("MIXRELAY_APPLICATION_CLOCK_FIXTURE").as_deref() == Ok("1");
    if live_fixture {
        println!("cargo:rustc-cfg=mixrelay_application_clock_fixture");
        cc::Build::new()
            .file(format!(
                "{native_root}/tests/MRApplicationClockLiveFixture.c"
            ))
            .include(&include)
            .include(&engine_include)
            .include(&hal_include)
            .flag("-std=c17")
            .flag("-fblocks")
            .flag("-Wall")
            .flag("-Wextra")
            .flag("-Werror")
            .compile("mixrelay_application_source_transport");
    }

    println!("cargo:rustc-link-lib=framework=CoreAudio");
    println!("cargo:rustc-link-lib=framework=AppKit");
    println!("cargo:rustc-link-lib=framework=Foundation");
    // `tauri dev` launches the bare Mach-O instead of an application bundle. Embed the
    // privacy usage description and stable bundle identity in that executable as well, so
    // Core Audio process-tap permission is attributed and prompted exactly like the bundled app.
    println!(
        "cargo:rustc-link-arg=-Wl,-sectcreate,__TEXT,__info_plist,{}",
        info_plist.display()
    );
    println!("cargo:rerun-if-changed={}", info_plist.display());
    println!("cargo:rerun-if-changed={native_root}/include/MRAudioRing.h");
    println!("cargo:rerun-if-changed={native_root}/include/MRApplicationSourceTransport.h");
    println!("cargo:rerun-if-changed={native_root}/include/MRProcessTap.h");
    println!("cargo:rerun-if-changed=../../../native/macos/common/MRCallbackClock.h");
    println!("cargo:rerun-if-changed={native_root}/src/MRApplicationSourceTransport.c");
    println!("cargo:rerun-if-changed={native_root}/tests/MRApplicationClockLiveFixture.c");
    println!("cargo:rerun-if-changed={engine_include}/MREngineSourceRegistry.h");
    println!("cargo:rerun-if-changed={engine_include}/MREngineControl.h");
    println!("cargo:rerun-if-changed={engine_include}/MRApplicationClock.h");
    println!("cargo:rerun-if-changed={hal_include}/MRHALControlProtocol.h");
    println!("cargo:rerun-if-changed={hal_include}/MRSharedAudioBuffer.h");
}
