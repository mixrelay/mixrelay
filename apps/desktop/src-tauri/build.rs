fn main() {
    println!("cargo:rustc-check-cfg=cfg(tonenari_application_clock_fixture)");
    println!("cargo:rerun-if-env-changed=TONENARI_APPLICATION_CLOCK_FIXTURE");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        build_macos_application_capture();
    }
    tauri_build::build();
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

    let live_fixture = std::env::var("TONENARI_APPLICATION_CLOCK_FIXTURE").as_deref() == Ok("1");
    let mut ring = cc::Build::new();
    if live_fixture {
        println!("cargo:rustc-cfg=tonenari_application_clock_fixture");
        ring.file(format!(
            "{native_root}/tests/TNApplicationClockLiveFixture.c"
        ));
    } else {
        ring.file(format!("{native_root}/src/TNApplicationSourceTransport.c"));
    }
    ring.file(format!("{native_root}/src/TNAudioRing.c"))
        .include(&include)
        .include(&engine_include)
        .include(&hal_include)
        .flag("-std=c17")
        .flag("-fblocks")
        .flag("-Wall")
        .flag("-Wextra")
        .flag("-Werror")
        .compile("tonenari_audio_ring");

    cc::Build::new()
        .files([
            format!("{native_root}/src/TNAudioDeviceDiscovery.m"),
            format!("{native_root}/src/TNProcessTap.m"),
        ])
        .include(&include)
        .flag("-fobjc-arc")
        .flag("-mmacosx-version-min=14.2")
        .flag("-Wall")
        .flag("-Wextra")
        .flag("-Werror")
        .compile("tonenari_process_tap");

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
    println!("cargo:rerun-if-changed={native_root}/include/TNAudioRing.h");
    println!("cargo:rerun-if-changed={native_root}/include/TNApplicationSourceTransport.h");
    println!("cargo:rerun-if-changed={native_root}/include/TNAudioDeviceDiscovery.h");
    println!("cargo:rerun-if-changed={native_root}/include/TNProcessTap.h");
    println!("cargo:rerun-if-changed=../../../native/macos/common/TNCallbackClock.h");
    println!("cargo:rerun-if-changed={native_root}/src/TNAudioRing.c");
    println!("cargo:rerun-if-changed={native_root}/src/TNApplicationSourceTransport.c");
    println!("cargo:rerun-if-changed={native_root}/tests/TNApplicationClockLiveFixture.c");
    println!("cargo:rerun-if-changed={engine_include}/TNEngineSourceRegistry.h");
    println!("cargo:rerun-if-changed={native_root}/src/TNAudioDeviceDiscovery.m");
    println!("cargo:rerun-if-changed={native_root}/src/TNProcessTap.m");
    println!("cargo:rerun-if-changed={engine_include}/TNEngineControl.h");
    println!("cargo:rerun-if-changed={engine_include}/TNApplicationClock.h");
    println!("cargo:rerun-if-changed={hal_include}/TNHALControlProtocol.h");
    println!("cargo:rerun-if-changed={hal_include}/TNSharedAudioBuffer.h");
}
