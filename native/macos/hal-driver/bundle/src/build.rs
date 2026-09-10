use std::{error::Error, ffi::OsStr, fs, path::Path, process::Command};

use super::command::{output, run};

const DEPLOYMENT_TARGET: &str = "14.2";
const SLICES: [(&str, &str); 2] = [
    ("arm64", "aarch64-apple-darwin"),
    ("x86_64", "x86_64-apple-darwin"),
];

pub(super) fn bundle(
    root: &Path,
    destination: &Path,
    identity: &OsStr,
) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(destination)?;
    let destination = destination.canonicalize()?;
    let bundle = destination.join("MixRelayHAL.driver");
    let slices = destination.join("slices");
    let executable = bundle.join("Contents/MacOS/MixRelayHAL");
    fs::create_dir_all(bundle.join("Contents/MacOS"))?;
    fs::create_dir_all(bundle.join("Contents/Resources"))?;
    fs::create_dir_all(&slices)?;
    let sdk = output(Command::new("xcrun").args(["--sdk", "macosx", "--show-sdk-path"]))?;
    for (architecture, target) in SLICES {
        build_slice(root, &slices, &sdk, architecture, target)?;
    }
    run(Command::new("xcrun")
        .args(["lipo", "-create"])
        .args(SLICES.map(|(arch, _)| slices.join(format!("MixRelayHAL-{arch}"))))
        .arg("-output")
        .arg(&executable))?;
    let driver = root.join("native/macos/hal-driver/driver");
    fs::copy(
        driver.join("Info.plist"),
        bundle.join("Contents/Info.plist"),
    )?;
    fs::copy(
        driver.join("LICENSE.apple-null-audio.txt"),
        bundle.join("Contents/Resources/LICENSE.apple-null-audio.txt"),
    )?;
    let architectures = output(
        Command::new("xcrun")
            .args(["lipo", "-archs"])
            .arg(&executable),
    )?;
    verify_architectures(&architectures)?;
    verify_factory(&executable)?;
    run(Command::new("plutil")
        .arg("-lint")
        .arg(bundle.join("Contents/Info.plist")))?;
    run(Command::new("codesign")
        .args(["--force", "--sign"])
        .arg(identity)
        .arg(&bundle))?;
    run(Command::new("codesign")
        .args(["--verify", "--strict"])
        .arg(&bundle))?;
    #[cfg(target_os = "macos")]
    super::probe::factory(&executable)?;
    println!(
        "Rust HAL bundle: {} ({architectures}); not installed",
        bundle.display()
    );
    Ok(())
}

fn build_slice(
    root: &Path,
    slices: &Path,
    sdk: &str,
    architecture: &str,
    target: &str,
) -> Result<(), Box<dyn Error>> {
    // Explicit target-dir prevents an inherited Cargo target-dir from silently
    // selecting an unrelated or stale archive during the native link step.
    let target_directory = root.join("target");
    run(Command::new("cargo")
        .current_dir(root)
        .args([
            "build",
            "--locked",
            "-p",
            "mixrelay-hal-core",
            "--release",
            "--target",
            target,
        ])
        .arg("--target-dir")
        .arg(&target_directory)
        .env("MACOSX_DEPLOYMENT_TARGET", DEPLOYMENT_TARGET))?;
    let mut compiler = link_command(root, &target_directory, architecture, target);
    compiler
        .args(["-isysroot", sdk, "-o"])
        .arg(slices.join(format!("MixRelayHAL-{architecture}")));
    run(&mut compiler)
}

fn link_command(root: &Path, target_directory: &Path, architecture: &str, target: &str) -> Command {
    let hal = root.join("native/macos/hal-driver");
    let mut compiler = Command::new("xcrun");
    compiler
        .args([
            "clang",
            "-std=c17",
            "-fblocks",
            "-bundle",
            "-arch",
            architecture,
            "-mmacosx-version-min=14.2",
            "-Wall",
            "-Wextra",
            "-Wpedantic",
            "-Wconversion",
            "-Wsign-conversion",
            "-Wshadow",
            "-Werror",
        ])
        .arg("-I")
        .arg(hal.join("include"))
        .arg("-I")
        .arg(hal.join("rust/include"))
        // Only Apple's COM/dispatch-block bridge is compiled as C. The static
        // archive owns properties, clocks, transport and all device lifetimes.
        .arg(hal.join("driver/MixRelayHALBridge.c"))
        .arg(
            target_directory
                .join(target)
                .join("release/libmixrelay_hal_core.a"),
        )
        .args([
            "-framework",
            "CoreAudio",
            "-framework",
            "CoreFoundation",
            "-liconv",
            "-Wl,-exported_symbol,_MixRelayHAL_Create",
        ]);
    compiler
}

fn verify_architectures(value: &str) -> Result<(), Box<dyn Error>> {
    let mut actual: Vec<_> = value.split_whitespace().collect();
    actual.sort_unstable();
    if actual != ["arm64", "x86_64"] {
        return Err(format!("expected exactly arm64 and x86_64, found {value}").into());
    }
    Ok(())
}

fn verify_factory(executable: &Path) -> Result<(), Box<dyn Error>> {
    for (architecture, _) in SLICES {
        let symbols = output(
            Command::new("xcrun")
                .args(["nm", "-arch", architecture, "-gU"])
                .arg(executable),
        )?;
        let exports: Vec<_> = symbols
            .lines()
            .filter_map(|line| line.split_whitespace().last())
            .collect();
        if exports != ["_MixRelayHAL_Create"] {
            return Err(format!("unexpected {architecture} plugin exports: {exports:?}").into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn universal_bundle_rejects_missing_duplicate_and_unexpected_slices() {
        assert!(verify_architectures("x86_64 arm64").is_ok());
        for wrong in ["arm64", "x86_64", "arm64 arm64", "arm64 x86_64 arm64e", ""] {
            assert!(verify_architectures(wrong).is_err());
        }
    }

    #[test]
    fn production_link_uses_rust_archive_and_only_abi_bridge_c() {
        let command = link_command(
            Path::new("/repo"),
            Path::new("/repo/target"),
            "arm64",
            "aarch64-apple-darwin",
        );
        let args: Vec<PathBuf> = command.get_args().map(PathBuf::from).collect();
        let c_sources: Vec<_> = args
            .iter()
            .filter(|arg| arg.extension() == Some(OsStr::new("c")))
            .collect();
        assert_eq!(
            c_sources,
            [&PathBuf::from(
                "/repo/native/macos/hal-driver/driver/MixRelayHALBridge.c"
            )]
        );
        assert!(args.contains(&PathBuf::from(
            "/repo/target/aarch64-apple-darwin/release/libmixrelay_hal_core.a"
        )));
    }
}
