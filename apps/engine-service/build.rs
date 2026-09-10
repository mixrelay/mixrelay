#[path = "../../crates/audio-io/src/formats.rs"]
pub mod formats;
#[path = "../../crates/audio-io/native_format_header.rs"]
mod native_format_header;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        build_macos_service();
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        build_pipewire_service();
    }
}

fn build_pipewire_service() {
    let library = pkg_config::Config::new()
        .atleast_version("0.3.65")
        .probe("libpipewire-0.3")
        .expect("install the PipeWire development package (libpipewire-0.3-dev / pipewire)");
    let root = "../../native/linux/pipewire";
    cc::Build::new()
        .includes(library.include_paths)
        .include(root)
        .include("../../native/pcm-ring/include")
        .flag("-std=c17")
        .define("_GNU_SOURCE", None)
        .warnings_into_errors(true)
        .file(format!("{root}/MRPipeWire.c"))
        .compile("mixrelay_pipewire_native");
    println!("cargo:rerun-if-changed={root}");
    println!("cargo:rerun-if-changed=../../native/pcm-ring/include/MRPcmRing.h");
}

fn build_macos_service() {
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    native_format_header::write_header(&output).expect("generate native audio format declarations");
    let engine_root = "../../native/macos/engine-service";
    let hal_root = "../../native/macos/hal-driver";
    let engine_include = format!("{engine_root}/include");
    let hal_include = format!("{hal_root}/include");
    let sources = [format!("{engine_root}/src/MREngineService.c")];
    let mut build = cc::Build::new();
    build
        .include(&output)
        .include(&engine_include)
        .include("../../native/pcm-ring/include")
        .include(&hal_include)
        .flag("-std=c17")
        .flag("-fblocks")
        .flag("-mmacosx-version-min=14.2")
        .warnings_into_errors(true)
        .files(&sources)
        .compile("mixrelay_engine_service_native");

    for source in sources {
        println!("cargo:rerun-if-changed={source}");
    }
    println!("cargo:rerun-if-changed={engine_include}/MREngineControl.h");
    println!("cargo:rerun-if-changed={engine_include}/MRApplicationClock.h");
    println!("cargo:rerun-if-changed={engine_include}/MRDeviceIO.h");
    println!("cargo:rerun-if-changed={engine_include}/MRAggregateGroup.h");
    println!("cargo:rerun-if-changed=../../native/macos/common/MRCallbackClock.h");
    println!("cargo:rerun-if-changed=../../native/macos/common/MRAnonymousMemory.h");
    println!("cargo:rerun-if-changed=../../native/macos/common/MRDeviceBuffers.h");
    println!("cargo:rerun-if-changed=../../crates/audio-io/src/formats.rs");
    println!("cargo:rerun-if-changed=../../crates/audio-io/native_format_header.rs");
    println!("cargo:rerun-if-changed=../../native/pcm-ring/include/MRPcmRing.h");
    println!("cargo:rerun-if-changed={engine_include}/MREndpointPeers.h");
    println!("cargo:rerun-if-changed={engine_include}/MREngineSourceRegistry.h");
    println!("cargo:rerun-if-changed={hal_include}/MRHALControlProtocol.h");
    println!("cargo:rerun-if-changed={hal_include}/MREndpointTransport.h");
    println!("cargo:rerun-if-changed={hal_include}/MRSharedAudioBuffer.h");
    println!("cargo:rustc-link-lib=framework=CoreAudio");
}
