# macOS MixRelay packaging

MixRelay is one product: one public application, one version, one complete installer
and one update catalog. The installer always includes the editor, background audio,
virtual-device service and HAL. There is no Driver selection or separate Driver app.
The Cargo workspace version is canonical; `pnpm product:version check` verifies the
application manifests and both HAL version fields. The unreleased product stays at 0.1.0.

## Build

```sh
pnpm signing:check
pnpm release:macos --output /absolute/empty/release-directory
```

The project signing tool unlocks the restored private build keychain, verifies real
Application and Installer signatures, keeps that keychain unlocked for the build,
and restores the original search list on exit. See [development rules](../../docs/development.md).
Credentials and private keys are not repository inputs. Release inputs require
Developer ID Application signing; there is no unsigned release fallback.

The release command builds Universal 2 editor, engine, device service, HAL,
installer CLI, payload checker and removal app. It prepares a fresh private payload,
verifies signatures and architecture slices, checks that the app, HAL, remover and
both services report the same product version, and produces
`MixRelay-Installer-VERSION.pkg` signed with the matching Developer ID Installer.
It submits that final PKG with `xcrun notarytool submit --wait`, requires an
`Accepted` verdict, then runs `xcrun stapler staple` and `xcrun stapler validate`.
The private signing config must include one `notarization` object:

```json
{ "notarization": { "keychainProfile": "YOUR_NOTARYTOOL_PROFILE" } }
```

Alternatively, use `notarization.apiKeyPath` (an absolute `.p8` path), `keyId`
and `issuer` for an App Store Connect team API key. These fields reference
credentials; never put key contents or inline passwords in the config or repository.
Missing or conflicting references stop the release before signing.

`--package-only` reuses compiled signed inputs and prepares a fresh payload.
`--skip-notarization` is for local test builds and records `notarized: false`.
It skips submission, stapling and validation; its PKG is not ready for publication.
The JSON result is printed and saved as `build-result.json` in the output directory,
with submission ID, Apple status and separate notarization, staple and validation
outcomes. Failures with a submission ID fetch `notarization-log.json`; a failed log
fetch is recorded without hiding the release failure. Credential references and raw
tool diagnostics are omitted from the result and console output.
Interruption stops the owned packaging process group before the signing wrapper
restores the keychain search list. A terminated process may leave a pending step
in the result; only `notarized-artifacts-built` with all three checks true qualifies.
Build staging is removed after success or failure. These commands do not install
or publish. Notarization and stapling remain required before publication.

`pnpm desktop:build` builds the universal public app and its native helpers.
Low-level staging is available through
`scripts/prepare-macos-release-payload.mjs --help`; every input, including the
engine and removal app, must be explicit. Staging alone is not release qualification.

## Installation

The fixed owned targets are:

- `/Applications/MixRelay.app` (`io.mixrelay.desktop`)
- `/Applications/Uninstall MixRelay.app`
- `/Library/Application Support/MixRelay/bin/mixrelay-driver-service`
- `/Library/Application Support/MixRelay/bin/mixrelay-engine-service`
- `/Library/Audio/Plug-Ins/HAL/MixRelayHAL.driver` (`io.mixrelay.audio.driver`)
- `/Library/LaunchDaemons/io.mixrelay.audio.driver.plist`
- `/Library/LaunchDaemons/io.mixrelay.audio.engine.plist`
- `/Library/LaunchDaemons/io.mixrelay.driver.removal.plist`

The package has one fixed component, `customize="never"`, no optional Driver,
no removal choice and no Installer plug-in. Its signed native transaction verifies
all inputs and each present installed target. A missing owned component can be
repaired; foreign code at an expected path is rejected.

App data and the runtime's control socket live under `io.mixrelay.desktop`, device
settings under `io.mixrelay.driver`.
Updates keep service labels, sockets, lifecycle leases and the HAL identity.
The installer does not rename or delete devices, graphs, settings, recordings or
authentication. It preserves the original OS-verified installation account and
binds service HOME, user name and group to that account. Switching login accounts
while installed does not transfer data or service ownership.

The root-only CLI requires the explicit `--reload-coreaudio` flag before mutation.
After administrator authorization and verification, it observes and requests
acknowledged native Quit from editor and background roles in the installation
user's session. Each process is identified by its kernel audit token (PID and PID
version, `process_exit`) before Quit, and has quit when its PID no longer yields that
token; `NSRunningApplication.isTerminated` is not used for this because a
command-line installer may never observe it change. Quit gets 100 seconds, then
remaining processes receive `SIGTERM` through the same token, so a reused PID is
never signalled (the runtime routes it through the same acknowledged shutdown),
within a 130-second total. App Quit errors name the role and PID. The Driver service's
identity is captured before `bootout` for the same reason. It acquires `installer.owner` and the fixed process-owner leases
before replacement. Installer's `must-close` applies to the removal app; it must
not erase the running-state observation before the transaction begins.

Commit stops verified audio jobs, waits for their exact processes to exit,
replaces all owned payloads, registers listeners and checks actual readiness,
version and process identity. It reloads CoreAudio through the OS process-signal
path; `launchctl kickstart -k` is not supported with SIP enabled. No reboot is
required. HAL absence/presence is checked in a bounded child of the signed CLI;
failed or timed-out CoreAudio checks do not count as success. The same read-only
check runs before file mutation. `mixrelay-macos-installer check-coreaudio`
requires neither installation nor administrator authentication.

On failure, the transaction drains new services, restores files and registrations,
reloads the restored HAL when needed and reports incomplete recovery as failure.
It reopens only roles it actually closed, after verified commit or successful
rollback, under the original UID. A fresh install does not launch previously
stopped apps. Private rollback staging is cleaned under retained leases; cleanup failure is
reported. No unrelated service or user document is removed.

Failed-install recovery checks the dependencies of the roles it actually closed:

| Role | Required installed targets before reopening |
| --- | --- |
| Editor | App, engine executable, engine launchd definition |
| Background runtime | App, Driver executable, HAL, Driver launchd definition, engine executable, engine launchd definition |

If both roles closed, their combined dependencies must verify before either is
reopened. Other missing targets do not prevent editor-only recovery; present
targets still undergo signature and payload verification. The engine launchd
definition is required because the editor connects to the installed fixed-name
engine service. A successful file rollback alone is insufficient.

## Runtime and removal

The public editor and internal `--driver-runtime` role use the same MixRelay bundle.
Closing the editor or a window keeps audio running. Explicit runtime Quit stops
devices and saves the inactive state before exiting. The service remains available
and does not revive devices merely because launchd restarts it. `drivers.json`
preserves device geometry and directional formats and records the explicit stop.
Reopening MixRelay restores the retained inventory. A failed stop/save leaves the
runtime menu available and shows an error.

`Uninstall MixRelay.app` removes the complete product through the named
`io.mixrelay.driver.remove` Authorization Services right and fixed signed XPC
helper. The normal-user app does not request general privileged execution.
Both peers enforce exact code identities and the matching publisher. The helper
checks the granted right, closes the observed app roles, and uses the same verified
transaction and rollback guards. Partial removals still verify every present target.
If the Driver plist is missing, fixed lifecycle lease ownership resolves the
installation account. Absent targets are successful no-ops.

Saved device settings deletion is default-off and deletes only `drivers.json` for
the original installation account. Graphs, recordings,
exported work and authentication remain. Cancellation changes no product state.
Committed success requires a verified reply, absent owned paths and completed
helper retirement. The helper executes a verified private copy until the client
acknowledges completion, so deleting its installed bundle cannot invalidate the
reply signature. Cleanup failures prevent success. Fixed listener identity and
bounded abandoned-completion handling remain unchanged.

## Installer resources and qualification

Installer shows both App and Driver component agreements through its native
Continue → Agree flow. All eleven languages are defined in
`packaging/localization/languages.json`; resources exist only in their native
language directories so unlocalized copies cannot shadow the user's language.
The original plain agreements, CHANGELOG and third-party notices remain bundled.
RTF rendering rejoins source column wraps and escapes Unicode without removing
legal words, contacts or agreement versions. Each agreement retains its own title
and subtitle. The readme explains complete-product installation, preserved data,
brief system-audio interruption and no reboot. Recording/call apps should be quit
before installing. Internal legal review is in `docs/legal-review.md`.

Source fixtures, signed builds and package inspection are separate from actual
installation qualification. The final artifact still needs clean install, update
and rollback, real HAL/device readback, graph/recording/authentication
preservation, editor-exit continuity, explicit stop, removal and reinstall checks.
Record those results in [the product checklist](../../docs/product.md).
The optional move-to-Trash prompt belongs to macOS Installer; test it through a
real browser download to Downloads. Postinstall never deletes the downloaded PKG.
