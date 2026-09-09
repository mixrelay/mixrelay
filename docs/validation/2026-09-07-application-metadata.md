# Application names, icons and selection-safe presentation — 2026-09-07

## Existing omissions repaired

Windows/Linux inventory returned `icon_data_url: None`. Windows displayed only an
executable basename. Independently, the shared UI matched node icons only by a
macOS bundle selector, so returning native icons alone would not have displayed
them on PID-selected Windows/Linux nodes. Menu IDs also collided when multiple
processes shared an application ID.

- Windows reads the limited-query executable path, version FileDescription and
  a real shell icon. The icon is rendered to a 32×32 transparent PNG, including
  legacy masks, with GDI writes flushed before CPU read. Process, icon, DC and DIB
  handles have scoped ownership and restore/delete ordering.
- Windows metadata cache is bounded to 128 entries, expires after 60 seconds and
  checks file modification time. No shared cache lock spans filesystem or Shell
  I/O. Protected/exited processes keep their original name/generic-icon fallback.
- Linux resolves bounded XDG/Flatpak desktop entries, localized Name and explicit
  PipeWire icon names through hicolor/Adwaita/pixmap PNG locations. Entries are
  never executed. Regular-file checks, nonblocking opens, 64 KiB file limits,
  1024-pixel dimensions and a 4 MiB decoder limit protect metadata reads.
- Linux PNGs are fully frame-decoded before exposure; invalid CRC/truncated-image
  tests replaced the insufficient header-only validation. Its 64-entry cache
  expires positive and negative results after 30 seconds; metadata lookup per
  inventory is capped at 64 distinct processes. Additional processes remain in the
  list with their original metadata fallback, rather than disappearing.
- UI icon matching follows bundle, process or explicit executable-path selection.
  Names are never used to guess another PID. Menu selections for two instances of
  one application are distinct. Rendering changes do not modify saved capture
  selectors, gain, revision or user labels. Missing/replaced targets clear stale
  icons; failed image decoding falls back to the node's application glyph.

## Validation boundary

- UI: **196 passed / 33 files**, TypeScript/Vite build and Core boundaries passed.
  Eight new tests cover exact selector identity, duplicate application IDs, old
  bundle inventory compatibility, unchanged runtime config, disappearance, malformed
  selectors and image decoding fallback/replacement. The existing bundle warning
  remains; no bundle performance improvement is claimed.
- macOS: **100 changed-boundary common Rust tests + 4 desktop proxy tests** passed
  locally; selected-crate Clippy and formatting passed.
  Windows/Linux-only metadata modules are not qualified by a macOS build.
- Ubuntu ARM64: frozen `app-metadata-v3` source; engine build, **155 passed /
  1 opt-in ignored**, and Clippy passed. An additional explicit read-only installed
  desktop probe passed: `apport-gtk` resolved `Report a problem...` and a valid PNG
  data URL of 5222 bytes from installed files. No application was launched.
- Windows ARM64: frozen `windows-app-metadata-v5`; seven-package build,
  **158 passed / 7 ignored**, and Clippy passed (exit 0). Tests read actual
  `cmd.exe` FileDescription and decode its 32-pixel PNG, preserve the current
  process PID selector, and cover access/missing-process fallbacks.
  An additional isolated native resource test passed: after warm-up, **100 icon
  and process-query cycles** left process/GDI/USER handles unchanged,
  **(208, 40, 0) → (208, 40, 0)**. This is a bounded lifetime regression check,
  not a whole-app leak or performance claim. The seven ignored tests were not
  executed by this run and are not counted as passes.

Ubuntu source archive SHA-256:
`404efbc2740b1a5e3d2ad7d9dd033cd9ae0be1f905d3dc9ab1819bc8ab289745`.
Evidence lives below
`/Volumes/Linear 1/test/tonenari/evidence/native-device-parity-20260907/`.
The `linux-integration/app-metadata-v3-*` files retain output and the explicit
exit code. Earlier build/style failures are retained, not counted as passes.

Windows evidence is in `windows-app-metadata-jF1k98/` below the same evidence
root: `v5-output.log`, `v5-exit-code` (`0`), `v5-manifest.json`, and
`v5-source.tar.gz`. All **76 source hashes** were checked on the guest before
building and independently matched against the current host checkout afterward.
Archive SHA-256:
`0cc75ba3e8f6028d252039883633cbb631f9c14bb66445b968ab715f3f5bb312`.
Earlier snapshot/setup and Clippy failures remain preserved.

## Not completed by this patch

Capture still uses the existing PID selector on Windows/Linux. Executable path or
desktop ID is metadata, not an implemented app-relaunch subscription. Automatic
new-PID/new-stream retracking remains open; preserving selection intent requires
the native capture owner to participate, not a UI name match.

Linux custom/SVG-only icon themes, unsupported desktop-ID mappings and OS desktop
session differences retain honest fallbacks. Installed Ubuntu metadata does not
qualify Debian/Kali/Arch or Linux x86_64. Actual source-menu/native-window visual
acceptance remains separate. No service/driver/device/default/security changes or
audio routing tests are performed by these metadata checks. Bounded resource
checks do not establish whole-process leak freedom or an OS-wide Shell timeout.
