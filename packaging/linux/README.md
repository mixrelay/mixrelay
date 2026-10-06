# Linux Driver packaging

MixRelay Driver is the `mix-relay-driver` DEB/RPM product. It includes the free
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
Updates require MixRelay, its graph engine and the Driver manager to be closed.
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
