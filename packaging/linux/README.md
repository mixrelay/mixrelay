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

`--bundles rpm` selects RPM. Tauri consumes the canonical `tauri.conf.json` and
`tauri.linux.conf.json`; it prepares both matching native services and records
`/usr/share/mixrelay-driver/version.json` only when the Cargo and desktop versions
agree. The inspection CLI accepts only the `mix-relay` package and requires all
three executables, the device service unit, session activation, policy, remover,
complete agreements and a matching version receipt. It checks ownership,
permissions, bounded ELF headers, dependencies and exact lifecycle hooks without
executing or installing payloads.

Use the distribution package manager to resolve dependencies:

```sh
sudo apt install ./MixRelay_VERSION_ARCH.deb
sudo dnf install ./MixRelay-VERSION-1.ARCH.rpm
sudo pacman -U ./mix-relay-VERSION-1-x86_64.pkg.tar.zst
```

The native dependencies include GUI/audio libraries, PipeWire >= 1.2 and WirePlumber >= 0.5,
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

The script builds the `mix-relay` package. The recipe pins payload SHA-256 and
disables stripping to preserve inspected executable bytes. It needs Debian
inspection tools on the preparation host and normal Arch build tools when running
`makepkg`.

## Ownership and lifecycle

The package owns the editor, both services, WirePlumber policy and remover.
There is one public launch entry and one public uninstall entry.

The user service is `/usr/lib/systemd/user/mixrelay-driver.service`. A hidden XDG
autostart entry starts it at login. Installation reloads signed-in users' managers
and starts their existing user units; audio never runs as root or through a
competing global service. An explicit app connection or engine recovery starts the
same user unit before launching the engine, including when login activation has
not run. A failed or unresponsive service start fails that connection within ten
seconds. Closing the editor or its window keeps audio running.
Explicit runtime Quit stops devices and persists the inactive state with the device
settings; service restart alone does not reactivate them. Reopening MixRelay
restores the saved devices. Failed stop/save keeps the runtime available and
reports an error.

Package lifecycle hooks reject observed active editor and background processes,
stop only owned user services and wait for their exact processes before replacing
files. Install/upgrade starts the existing user service after replacement.
Removing the previous version during an upgrade does not stop the new service.
Removal reloads user service definitions after deleting the unit and autostart
entry. Arch's package-scoped PreTransaction hook uses AbortOnFail to reject
active-process upgrade/removal before file changes.

Device settings are stored at `$XDG_CONFIG_HOME/mixrelay-driver/drivers.json`, or
under `~/.config/mixrelay-driver` when `XDG_CONFIG_HOME` is unset. Installation and
update do not delete device settings, app graphs, settings, recordings or
authentication. Closing the editor does not change device or runtime ownership.

The public remover removes `mix-relay` through the package manager, using the
session's registered polkit authentication agent; MixRelay does not start its own
agent. It then clears the paid-license sign-in. The choice to delete saved graphs
and settings is default-off and applies only after successful removal, to fixed
application-owned paths that pass a symlink-free path check. Recordings, exported
projects and trial protection remain. Direct package-manager removal preserves all
per-user settings.

The installed desktop entry starts the user's Driver unit before starting its
owned engine. Controller IPC uses `$XDG_RUNTIME_DIR/mixrelay/control/control.sock`;
persistent data and its ancestors retain their existing permissions. The private
normal-user launcher check is documented in [the lab guide](../../infra/lab/README.md).

## Documents and qualification

All eleven App and Driver component agreements, CHANGELOG and third-party notices
ship together. The language/file mapping is `packaging/localization/languages.json`.
Menus and removal guidance support those languages. Chinese script identifiers
select the script; TW/HK/MO use Traditional Chinese, CN/SG or generic zh use
Simplified Chinese. Removal follows LC_ALL, LC_MESSAGES, then LANG and falls back
to English for unsupported locales.

Source contracts and metadata inspection do not qualify actual installation.
DEB, RPM and Arch still require distribution-specific clean install,
upgrade/removal, user-session audio and preserved device/graph/settings/
recording/authentication checks with the complete artifact. Record observed results
and remaining work in [the product checklist](../../docs/product.md).

The async graph contribution path requires PipeWire 1.2 scheduling; this is also
checked at build and connection preparation. The source-mute policy requires
WirePlumber 0.5. Debian dependencies declare these versions. Tauri emits RPM
requirements as literal capability names, so RPM retains native package names;
the linked PipeWire version is still checked before connection preparation. Ubuntu 24.04's
unmodified audio packages are too old; the lab's Ubuntu image uses 26.04.
