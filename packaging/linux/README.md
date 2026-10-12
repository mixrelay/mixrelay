# Linux MixRelay packaging

MixRelay is one `mix-relay` DEB/RPM/Arch product. One public `mixrelay-desktop`
application includes the editor, background audio and virtual-device management.
The package always includes `mixrelay-driver-service` and `mixrelay-engine-service`;
there is no Driver choice, separate manager or separate update catalog.
All components use the Cargo workspace version. The unreleased product stays at 0.1.0.

Audio runs in a normal signed-in user's PipeWire session, including x64 Proxmox
guests. Docker remains useful for builds and code tests; container audio and
Docker-backed audio environments are unsupported.

## Build and inspect

```sh
pnpm product:version check
pnpm desktop:build --bundles deb
node scripts/check-linux-package.mjs /absolute/MixRelay_VERSION_ARCH.deb
```

`--bundles rpm` selects RPM. `--driver` is retired. Tauri consumes the canonical
`tauri.conf.json` and `tauri.linux.conf.json`; it prepares both matching native
services and records `/usr/share/mixrelay-driver/version.json` only when the
Cargo and desktop versions agree. This stable internal receipt path is retained.
The product build and inspection CLI require all three executables, the device
service unit, session activation, policy, remover, complete agreements and matching
version receipt. A legacy editor-only payload cannot pass as a complete product.
Source validation retains explicit legacy fixture support for migration tests.
Inspection checks ownership, permissions, bounded ELF headers, dependencies and
exact current lifecycle hooks without executing or installing payloads.

Use the distribution package manager to resolve dependencies:

```sh
sudo apt install ./MixRelay_VERSION_ARCH.deb
sudo dnf install ./MixRelay-VERSION-1.ARCH.rpm
sudo pacman -U ./mix-relay-VERSION-1-x86_64.pkg.tar.zst
```

The native dependencies include GUI/audio libraries, PipeWire/WirePlumber,
systemd, plugin hosting and the session's normal authorization tools. End users
need no SDK or manual service configuration. AppImage is not the coordinated
installation/update path. The current release toolchain requires glibc 2.39;
verify actual ELF symbol requirements when changing that toolchain. RPM declares
`libc.so.6(GLIBC_2.39)(64bit)`, not a literal version-expression dependency.
Sidecar executables are explicitly mode 0755 before packaging.

Arch uses the same inspected full-product DEB payload:

```sh
node scripts/prepare-arch-driver-package.mjs INPUT.deb NEW_BUILD_DIRECTORY
cd NEW_BUILD_DIRECTORY
makepkg --noconfirm --clean
```

The script name is retained for compatibility; its package is `mix-relay`.
The recipe pins payload SHA-256 and disables stripping to preserve inspected
executable bytes. It needs Debian inspection tools on the preparation host and
normal Arch build tools when running `makepkg`.

## Ownership, migration and lifecycle

DEB replaces/conflicts with `mix-relay-driver`; RPM obsoletes/conflicts with it;
Arch replaces/conflicts with it. These are unversioned ownership migrations so
an unreleased version reset cannot strand a newer split package. The single
package owns the editor, both services, WirePlumber policy and full remover.
There is one public launch entry and one public uninstall entry. Stable internal
service, settings, policy, socket and receipt paths remain unchanged.

The user service is `/usr/lib/systemd/user/mixrelay-driver.service`. A hidden XDG
autostart entry starts it at login. Installation reloads signed-in users' managers
and starts their existing user units; audio never runs as root or through a
competing global service. The launch entry ensures the service is available before
opening MixRelay. Closing the editor or its window keeps audio running.
Explicit runtime Quit stops devices and persists inactive state; service restart
alone does not reactivate them. `drivers.json` retains the previous schema;
`drivers.json.stopped` records the stop. Reopening MixRelay restores retained
devices. Failed stop/save keeps the runtime available and reports an error.
An older service restored by rollback may ignore the stop marker.

Package lifecycle hooks reject observed active editor and background processes,
stop only owned user services and wait for their exact processes before replacing
files. Install/upgrade starts the existing user service after replacement.
Old-package removal during upgrade does not stop the new service. Removal reloads
user service definitions after deleting the unit and autostart entry. Arch's
package-scoped PreTransaction hook uses AbortOnFail to reject active-process
upgrade/removal before file changes.

Device settings remain at `$XDG_CONFIG_HOME/mixrelay-driver/drivers.json`, or the
existing `~/.config/mixrelay-driver` fallback. App graphs, settings, recordings and
authentication keep their existing storage locations. Installation and update do
not delete them. Closing the editor does not change device or runtime ownership.

The public remover removes `mix-relay` through the package manager, using the
session's registered authentication agent or its standard terminal prompt.
Debian requires `xterm | x-terminal-emulator`; RPM requires `xterm`. MixRelay does
not start its own authentication agent. The settings-deletion choice is default-off
and applies only after successful removal, to the current user's exact device
settings and stopped marker. It never recursively deletes a directory. Graphs,
recordings, exports and authentication remain. Direct package-manager removal
preserves all per-user settings.

## Documents and qualification

All eleven App and Driver component agreements, CHANGELOG and third-party notices
ship together. The language/file mapping is `packaging/localization/languages.json`.
Menus and removal guidance support those languages. Chinese script identifiers
select the script; TW/HK/MO use Traditional Chinese, CN/SG or generic zh use
Simplified Chinese. Removal follows LC_ALL, LC_MESSAGES, then LANG and falls back
to English for unsupported locales.

Source contracts and metadata inspection do not qualify actual installation.
DEB, RPM and Arch still require distribution-specific clean install, split-package
migration, upgrade/removal, user-session audio and preserved device/graph/settings/
recording/authentication checks with the complete artifact. Record observed results
and remaining work in [the product checklist](../../docs/product.md).
