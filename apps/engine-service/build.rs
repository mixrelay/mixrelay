#[path = "../../crates/audio-io/src/formats.rs"]
pub mod formats;
#[path = "../../crates/audio-io/native_format_header.rs"]
mod native_format_header;

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        build_macos_service();
    }
}

fn build_macos_service() {
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    native_format_header::write_header(&output).expect("generate native audio format declarations");
    embed_service_info_plist();
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

fn embed_service_info_plist() {
    let manifest_dir = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"),
    );
    let info_plist = manifest_dir
        .join("../../native/macos/engine-service/Info.plist")
        .canonicalize()
        .expect("resolve the engine privacy Info.plist");
    let info_plist_argument = service_info_plist_link_argument(&info_plist);
    println!("cargo:rerun-if-changed={}", info_plist.display());
    println!("{info_plist_argument}");
}

fn service_info_plist_link_argument(info_plist: &std::path::Path) -> String {
    format!(
        "cargo:rustc-link-arg-bin=mixrelay-engine-service=-Wl,-sectcreate,__TEXT,__info_plist,{}",
        info_plist.display()
    )
}

#[cfg(test)]
mod tests {
    use super::service_info_plist_link_argument;
    use std::path::Path;

    #[test]
    fn privacy_plist_link_arg_targets_only_the_engine_service_binary() {
        let argument = service_info_plist_link_argument(Path::new("/repo/native/Info.plist"));
        assert!(argument.starts_with(
            "cargo:rustc-link-arg-bin=mixrelay-engine-service=-Wl,-sectcreate,__TEXT,__info_plist,"
        ));
        assert!(argument.ends_with("/repo/native/Info.plist"));
    }
}
