# Linux Driver packaging

The Driver status menu's Quit stops all native devices and loopback while
retaining saved device settings and a persisted inactive run state. The control
service remains available and does not revive devices on its own restart.
The `drivers.json.stopped` sibling records the stop; `drivers.json` retains its
previous schema for rollback compatibility. An older service ignores that marker
and may start devices under its previous policy. Delete saved settings removes both.
Reopening MixRelay Driver restores the retained devices. This differs from
quitting the graph app or merely closing a window; failed stop/save requests keep
the manager open and show a localized error. Earlier GUI-quit continuity results
cover the previous policy and do not qualify this new explicit-stop behavior.

MixRelay Driver is the `mix-relay-driver` DEB/RPM/Arch product. It includes the free
device manager, `/usr/bin/mixrelay-driver-manager`, the native
`/usr/bin/mixrelay-driver-service`, original Driver terms, CHANGELOG and third-party
notices. Its actual installed release version is recorded in
`/usr/share/mixrelay-driver/version.json`.

The Driver service runs in each signed-in user's PipeWire session through
`/usr/lib/systemd/user/mixrelay-driver.service`. One XDG autostart entry starts the
unit at login. Package installation reloads existing user managers and starts the
same unit as those users; it never runs audio as root or enables a competing
global service. The Driver manager entry also starts this unit before opening
the GUI. Closing the manager leaves devices and basic loopback running.

`pnpm desktop:build --driver` combines
`apps/desktop/src-tauri/tauri.driver.conf.json` and
`tauri.driver.linux.conf.json` as a Tauri merge patch, removing App-owned files.
Optional `--bundles deb` or `--bundles rpm` selects the package format.
The prepare hook builds the actual matching
service sidecar and writes a receipt only when Cargo workspace and desktop
package versions match. DEB/RPM dependency declarations include native GUI/audio
libraries, PipeWire/WirePlumber, systemd and the normal package authorization tools.
No SDK, developer tool or manual service setup is required by end users.
AppImage is not this installation/update path.

Use the distribution package manager to resolve dependencies:
`sudo apt install ./MixRelayDriver_VERSION_ARCH.deb` or
`sudo dnf install ./MixRelayDriver-VERSION-1.ARCH.rpm`.
Arch uses `sudo pacman -U ./mix-relay-driver-VERSION-1-x86_64.pkg.tar.zst`.
Updates require MixRelay, its graph engine and the Driver manager to be closed.
The Driver package requires glibc 2.39 or newer, matching the highest glibc symbol
version observed in the release manager. Check the actual ELF version requirements
when changing the build toolchain; successful packaging alone does not prove
compatibility with every Debian or RPM distribution.
RPM declares `libc.so.6(GLIBC_2.39)(64bit)`, which glibc provides, rather than a
version expression interpreted as a literal dependency name by the bundler.
The service preparer fixes sidecar mode to 0755 before packaging so a restrictive
build umask cannot leave a root-owned executable inaccessible to its user service.
Hooks reject those active executables, stop only observed Driver user services,
and wait for their exact process to exit. After successful replacement, the
package starts the user service again. RPM/Debian old-version removal during an
upgrade does not stop the replacement. Removal reloads user service definitions
after the unit and XDG entry are removed.

Driver configuration is `$XDG_CONFIG_HOME/mixrelay-driver/drivers.json`, or
`~/.config/mixrelay-driver/drivers.json` when XDG_CONFIG_HOME is not absolute.
Updates and default removal preserve it.

**Uninstall MixRelay Driver** presents an unchecked **Delete saved Driver device
settings** checkbox and uses Polkit for fixed `mix-relay-driver` package removal.
Its desktop entry opens a terminal: `pkexec` uses the session's registered GUI
authentication agent when available, otherwise its standard terminal prompt.
Debian requires `xterm | x-terminal-emulator`; RPM requires `xterm`. The Driver
does not require or start a separate authentication agent, and removal does
not require logging out and back in after installation.
Only after successful removal does the selected choice delete the current
user's exact settings file; it never recursively deletes a directory. App graphs,
recordings, exported work and paid authentication remain. Package-manager
removal directly preserves all per-user settings.

Arch builds use the same inspected binary/document payload as the DEB release,
with native `makepkg` metadata and dependencies. Prepare and build it with:

```sh
node scripts/prepare-arch-driver-package.mjs INPUT.deb NEW_BUILD_DIRECTORY
cd NEW_BUILD_DIRECTORY
makepkg --noconfirm --clean
```

The preparer requires Debian inspection tools on the release build host; the
generated recipe requires only Arch's usual `makepkg` tools. End users install
the resulting package with pacman. The payload SHA-256 is pinned in PKGBUILD;
stripping is disabled so the inspected executable bytes remain intact.
Arch's package-scoped `PreTransaction` hook uses `AbortOnFail` to reject an
upgrade/removal while the manager or graph App is active, before changing files
([pacman hook reference](https://man.archlinux.org/man/alpm-hooks.5.en)).
Install/upgrade starts the existing user-session service; removal reloads user
managers after deleting the unit. Neither package format enables audio as root.

The graph App remains a separate `mix-relay` package and owns its desktop,
graph engine and app-specific WirePlumber policy. Its removal must preserve
`mix-relay-driver`. A future combined App installer must resolve/install the
Driver automatically in one user flow; Driver users do not need the graph App.

Run `node scripts/check-linux-package.mjs PATH.deb` to inspect the actual Debian
artifact without executing or installing it. It checks numeric ownership and
permissions, ELF architecture, dependencies, current lifecycle hooks, Driver
service/manager ownership, user-session activation files, original documents and
the version receipt against the package's real Version. It rejects App or graph
engine binaries in a Driver package. RPM and installed desktop/audio behavior
still require actual artifact verification on the intended distribution.

Current qualification results and remaining checks are in
[the product checklist](../../docs/product.md).

On 2026-10-10, the x86_64 packages were qualified on Debian 13, Fedora 44 and Arch
rolling VMs: installation, native manager Quit/reopen, active-manager removal
rejection, same-version replacement, stopped-state/settings preservation,
removal and reinstall. Fedora and Arch also passed native remover confirmation
with selected Driver settings/stop-marker deletion, using a temporary test-account
Polkit rule for the fixed package manager; that rule was removed afterward.
Fedora and Arch additionally passed two indexed PCM
client scopes each for 2-channel Float32 and 64-channel Float32/PCM16/PCM24/PCM32;
each scope compared 480,255 fresh frames with zero continuity errors. Debian's
previous PCM qualification remains recorded separately. This does not qualify
different-version upgrades, every derivative distribution or long-term hardware
operation. The virtualization scheduling known issue remains documented in
`docs/product.md`.
Artifacts are retained in `artifacts/linux/`; detailed reports and source hashes
are in `.lab/linux-release-20261010/`. These packages are prepared for direct
download; this work does not publish them to distribution repositories or AUR.


App and Driver menus, removal guidance and full agreements support English,
Japanese, Korean, Simplified Chinese, Traditional Chinese, French, German,
Spanish, Portuguese, Italian and Russian.
DEB, RPM and Arch package definitions retain all corresponding agreement files,
listed in `packaging/localization/languages.json`. Chinese script identifiers
select the script explicitly; TW/HK/MO without a script use Traditional Chinese,
and CN/SG or generic zh use Simplified Chinese. Removal messages follow
LC_ALL, LC_MESSAGES, then LANG; unsupported locales use English.
