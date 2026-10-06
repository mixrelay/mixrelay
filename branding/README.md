# MixRelay identity

`icon.png` is the approved brick-red logo master (diagonal channels and the large
central block), selected on 2026-09-10. Keep its shape, color, and white background.

`tray-white.svg` derives the white mark directly from `icon.png` for menu bars and
system trays. macOS treats it as a template and adapts its color to the menu bar.
It replaces the former `MR` tray title.
The SVG removes the neutral white background through a color-to-alpha mask and
crops the outer margins; it does not redraw the approved contours. Render it with
the Tauri CLI so its relative image reference resolves against this directory.
Render the mask at 512px first, then downsample that PNG to the tray sizes to keep
antialiased diagonal edges; applying the mask directly at small sizes clips them.

Run `pnpm desktop:icons` from the repository root to regenerate assets:

- `apps/desktop/src-tauri/icons/`: app, Dock, taskbar, installer, and platform icons.
- `apps/desktop/src-tauri/tray-icons/`: 32px and 64px monochrome tray icons, plus a
  512px transparent white PNG for reuse.

Tauri's development and build hooks run the same command automatically. The bundle
configuration explicitly selects the app icons; the tray embeds the 64px image at
compile time, so it does not depend on a runtime file path. Generated icons are
kept in the repository so direct Cargo builds can also consume them.
