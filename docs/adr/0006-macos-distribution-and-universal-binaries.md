# ADR 0006: Make every macOS executable Universal 2

## Status

Accepted for architecture support. The Mac App Store feature split remains a
distribution decision that must be resolved before submission.

## Context

ToneNari is expected to support both Apple silicon and Intel Macs. The desktop
application is not the only compiled artifact: the Rust executable, native
capture code, bundled libraries, helper processes, and Core Audio HAL plug-in
must all support both architectures.

Apple's Universal Binary guidance requires compiled code to contain `arm64` and
`x86_64` variants, and specifically includes apps, plug-ins, libraries, daemons,
and drivers in that requirement. A fat app executable cannot compensate for a
single-architecture native dependency.

There is also a distribution constraint which is separate from architecture:

- Apple's AudioServerPlugIn installation path is
  `/Library/Audio/Plug-Ins/HAL`.
- Mac App Store Review Guideline 2.4.5 requires a sandboxed, self-contained app
  bundle; it disallows installing code or resources in shared locations,
  downloading additional executable functionality, and escalating to root.

It follows that a conventional HAL installer and a fully self-contained Mac App
Store bundle are incompatible under the current published rules. This is an
inference from the two Apple requirements, not a claim that Apple has reviewed
ToneNari.

## Decision

- Release builds for macOS target Universal 2: exactly `arm64` and `x86_64`.
- The HAL plug-in is compiled once per architecture and merged with `lipo`.
- Build validation inspects executable files, not merely their containing app or
  plug-in directories.
- A missing or additional architecture is a build failure.
- The Tauri App Store build uses `--target universal-apple-darwin`.
- Native dependencies and future helper executables must pass the same
  architecture gate before they are added to a release bundle.
- Local development may run a native-architecture debug build, but it is not a
  releasable artifact.

The full virtual-device edition is designed for Developer ID signing,
notarization, and direct distribution. Before a Mac App Store submission, we
must choose one of these product outcomes:

1. a store-safe edition that omits installation-dependent HAL functionality;
2. a different Apple-supported virtual-device mechanism which is compatible
   with App Sandbox and App Review; or
3. direct commercial distribution for the full edition instead of the Mac App
   Store.

No build may silently label a reduced App Store edition as feature-equivalent to
the full ToneNari edition.

## Verification

The HAL build runs both architecture compilers, merges them, then checks the
actual executable with `lipo -archs`. The Tauri build pins the full Xcode SDK and
both Cargo linkers, then rejects any Mach-O inside the resulting app bundle that
does not contain exactly the two required slices.

```sh
pnpm hal:build:macos-universal
pnpm desktop:build:macos-universal
```

## References

- [Apple: Building a universal macOS binary](https://developer.apple.com/documentation/apple-silicon/building-a-universal-macos-binary)
- [Apple: Creating an Audio Server Driver Plug-in](https://developer.apple.com/documentation/coreaudio/creating-an-audio-server-driver-plug-in)
- [Apple: App Review Guidelines](https://developer.apple.com/app-store/review/guidelines/)
- [Tauri: App Store distribution](https://v2.tauri.app/distribute/app-store/)
