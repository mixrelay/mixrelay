use std::{
    env,
    path::{Path, PathBuf},
};

#[path = "../audio-io/src/formats.rs"]
pub mod formats;
#[path = "../audio-io/native_format_header.rs"]
mod native_format_header;

fn main() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let root =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory")).join("../..");
    println!("cargo:rerun-if-changed=../../native/macos");
    println!("cargo:rerun-if-changed=../../native/pcm-ring");
    println!("cargo:rerun-if-changed=../audio-io/src/formats.rs");
    println!("cargo:rerun-if-changed=../audio-io/native_format_header.rs");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    build_transport(&root, &output);
    native_format_header::write_header(&output).expect("generate native audio format declarations");
    build(
        &root,
        "fixture",
        "native/macos/engine-service/tests/TNDeviceIOLive.c",
        &[],
    );
    build(
        &root,
        "clock",
        "native/macos/common/tests/TNCallbackClockTests.c",
        &[],
    );
    build(
        &root,
        "hal",
        "native/macos/hal-driver/tests/TNHALBridgeTests.c",
        &["native/macos/hal-driver/driver/TNHALControlBlock.c"],
    );
    build(
        &root,
        "application_clock",
        "native/macos/engine-service/tests/TNApplicationClockTests.c",
        &[],
    );
    build(
        &root,
        "audio_ring",
        "native/macos/application-capture/tests/TNAudioRingTests.c",
        &[],
    );
    build(
        &root,
        "pcm_ring",
        "native/pcm-ring/tests/TNPcmRingTests.c",
        &[],
    );
}

fn build_transport(root: &Path, output: &Path) {
    cc::Build::new()
        .cargo_metadata(false)
        .out_dir(output)
        .file(root.join("native/macos/application-capture/src/TNApplicationSourceTransport.c"))
        .include(root.join("native/macos/hal-driver/include"))
        .flag("-std=c17")
        .flag("-fblocks")
        .flag("-mmacosx-version-min=14.2")
        .warnings_into_errors(true)
        .compile("tonenari_source_xpc");
    for (kind, suffix) in [("rlib", "rlib"), ("staticlib", "a")] {
        rust_library(
            root,
            RustSource {
                path: "native/pcm-ring/src/lib.rs",
                name: "tonenari_pcm_ring",
            },
            kind,
            &output.join(format!("libtonenari_pcm_ring.{suffix}")),
            &[],
        );
    }
    let pcm = format!(
        "tonenari_pcm_ring={}",
        output.join("libtonenari_pcm_ring.rlib").display()
    );
    let search = format!("dependency={}", output.display());
    for (kind, suffix) in [("rlib", "rlib"), ("staticlib", "a")] {
        rust_library(
            root,
            RustSource {
                path: "native/macos/common/lib.rs",
                name: "tonenari_macos_transport",
            },
            kind,
            &output.join(format!("libtonenari_macos_transport.{suffix}")),
            &["--extern", &pcm, "-L", &search],
        );
    }
    let dependency = format!(
        "tonenari_macos_transport={}",
        output.join("libtonenari_macos_transport.rlib").display()
    );
    rust_library(
        root,
        RustSource {
            path: "native/macos/hal-driver/rust/src/lib.rs",
            name: "tonenari_hal_core",
        },
        "staticlib",
        &output.join("libtonenari_hal_core.a"),
        &["--extern", &dependency, "-L", &search],
    );
}

#[derive(Clone, Copy)]
struct RustSource<'a> {
    path: &'a str,
    name: &'a str,
}

fn rust_library(root: &Path, source: RustSource<'_>, kind: &str, output: &Path, extra: &[&str]) {
    let name = source.name;
    let result = std::process::Command::new(env::var_os("RUSTC").expect("Cargo rustc"))
        .args(["--edition=2024", "--crate-type", kind, "--crate-name", name])
        .arg(root.join(source.path))
        .arg("--target")
        .arg(env::var_os("TARGET").expect("Cargo target"))
        .args(extra)
        .arg("-o")
        .arg(output)
        .output()
        .expect("compile Rust transport ABI for native fixtures");
    assert!(
        result.status.success(),
        "Rust native fixture dependency {name} failed:\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

fn build(root: &Path, name: &str, test: &str, sources: &[&str]) {
    let output_directory = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"));
    let output = output_directory.join(name);
    let compiler = cc::Build::new().cargo_metadata(false).get_compiler();
    let result = compiler
        .to_command()
        .current_dir(root)
        .arg("-I")
        .arg(&output_directory)
        .args([
            "-std=c17",
            "-fblocks",
            "-Wall",
            "-Wextra",
            "-Werror",
            "-g",
            "-fsanitize=address,undefined",
            "-mmacosx-version-min=14.2",
        ])
        .args([
            "-I",
            "native/macos/engine-service/include",
            "-I",
            "native/macos/hal-driver/include",
            "-I",
            "native/macos/hal-driver/rust/include",
            "-I",
            "native/macos/application-capture/include",
            "-I",
            "native/pcm-ring/include",
        ])
        .arg(test)
        .args(sources)
        .arg(output_directory.join(if name == "hal" {
            "libtonenari_hal_core.a"
        } else {
            "libtonenari_macos_transport.a"
        }))
        .arg(output_directory.join("libtonenari_pcm_ring.a"))
        .arg(output_directory.join("libtonenari_source_xpc.a"))
        .arg("-liconv")
        .args([
            "-framework",
            "CoreAudio",
            "-framework",
            "CoreFoundation",
            "-o",
        ])
        .arg(&output)
        .output()
        .expect("launch native test compiler");
    assert!(
        result.status.success(),
        "{name} failed to compile:\n{}",
        String::from_utf8_lossy(&result.stderr)
    );
    println!(
        "cargo:rustc-env=TN_NATIVE_{}={}",
        name.to_uppercase(),
        output.display()
    );
}
