# App and tray icons — 2026-09-10

The approved diagonal-channel brick-red logo is now `branding/icon.png`. Its
SHA-256 is `279f61ad56b05c8027fd9cda735fe9a2b33354c946d6e4145c95cf3e462e403b`,
identical to the selected image. The previous orange-wave SVG was removed.

`pnpm desktop:icons` generates platform app icons and a white transparent tray
mark derived from that same source through `branding/tray-white.svg`. The tray
mask crops the outer margins and renders at 512px before downsampling to 32/64px.
Tauri development and build hooks regenerate these assets. The desktop embeds
the 64px tray PNG and sets macOS template mode; Windows uses the white pixels.

## Verification

- `pnpm desktop:build --debug --bundles app`: passed, including icon generation,
  frontend type checking, Rust compilation, and macOS app packaging.
- Bundle `CFBundleIconFile` is `icon.icns`; its SHA-256 matches the generated
  `apps/desktop/src-tauri/icons/icon.icns`. Extracted bundled artwork was viewed.
- `cargo fmt -p mixrelay-desktop --check`: passed.
- `cargo clippy -p mixrelay-desktop --all-targets -- -D warnings`: passed.
- `cargo test -p mixrelay-desktop -- --test-threads=1`: 30 passed, none skipped.
- `cargo doc -p mixrelay-desktop --no-deps`: passed.
- `git diff --check`: passed.
- Independent review verified paths, build-hook working directories, macOS 18pt
  tray scaling, image embedding, and alpha quality. At 64px, 1,805 fully opaque
  pixels are pure white; 1,148 partially transparent pixels smooth the edges.
- Existing Vite bundle-size advisory remains; Rust compilation had no warnings.

This verifies the local macOS debug package and generated Windows ICO/PNG assets.
Windows compilation and native tray rendering on either OS were not exercised.
No installation, signing, publishing, or release was performed. Unrelated work
already present in the checkout was preserved.
