# macOS Driver packaging

MixRelay Driver installs its free device manager, native service and HAL in one
signed administrator-authorized transaction. The graph App is a separate
product; Driver install/update/remove never replaces its bundle, graph service,
graphs, recordings or paid authentication.

The native status menu lists devices directly, each with sample rate, bit depth
and channel-count settings. It also provides Check for Updates and Quit. Update
results stay in the menu; a validated download is offered only when available.
There is no update alert, App launcher or removal command in this menu.

The exact Driver targets are:

- `/Applications/MixRelay Driver.app` (`io.mixrelay.driver.desktop`)
- `/Library/Application Support/MixRelay/bin/mixrelay-driver-service`
- `/Library/Audio/Plug-Ins/HAL/MixRelayHAL.driver` (stable `io.mixrelay.audio.driver`)
- `/Library/LaunchDaemons/io.mixrelay.audio.driver.plist`
- `/Applications/Uninstall MixRelay Driver.app`
- `/Library/LaunchDaemons/io.mixrelay.driver.removal.plist`

The service uses the OS-verified installation user's HOME, short name and primary
group. Its fixed Mach service is `io.mixrelay.audio.driver`; RunAtLoad and KeepAlive
are true, so dismissing a window or a manager crash does not stop native devices.
Explicit Quit requests a native stop and saves the inactive state before the
manager exits; launchd keeps the control service available for the next start.
Driver settings live in that user's
`Library/Application Support/io.mixrelay.driver/drivers.json`.
Updates and removal preserve the original installation account. If the Driver
plist was already deleted, removal resolves that account from the remaining
fixed lifecycle lease owners. Absent components are successful removal no-ops;
each present component still requires its own ownership and publisher checks.
After complete
removal, a fresh installation can bind the idle process leases to its new account
without replacing their inodes or changing the previous user's data. Switching
accounts while the Driver is installed does not transfer its settings or ownership.

Build the actual Universal 2 Driver service and manager with the Driver product
build. The service preparer is `scripts/prepare-driver-package.mjs`; the manager
build is `pnpm desktop:build --driver`, which selects the checked universal
builder's Driver profile. It builds the installer CLI, payload checker and dedicated
removal app for both architectures and signs their Universal 2 outputs. The PKG
installs the removal app separately in Applications. Release inputs require
Developer ID Application signing; there is no unsigned release fallback.

Generated release files live under the ignored `artifacts/` directory:
`artifacts/macos/` holds signed component inputs and `artifacts/macos-pkg/`
holds the final PKG. Local `.lab/` reports retain source manifests, signing and
notarization results, runtime evidence and settings backups; intermediate
payload trees and expanded packages can be removed after qualification.
The retained PKG belongs to its recorded build sources. Refactoring source does
not update that binary; rebuild and sign it before publishing changed code.

In `native/macos/installer/src`, `native/service.rs` coordinates service changes,
`native/service/payload.rs` verifies installed files and account-bound metadata,
and `native/service/runtime.rs` handles activation and readiness. The removal
listener's temporary signed executable is owned by `removal/executable.rs`.
Recovery diagnostics describe rollback failures; they do not preserve a recovery
folder. The transaction attempts staging cleanup after every rollback outcome.

Stage prebuilt artifacts with:

```sh
node scripts/prepare-macos-release-payload.mjs \
  --manager "/absolute/MixRelayDriver.app" \
  --hal "/absolute/MixRelayHAL.driver" \
  --driver "/absolute/mixrelay-driver-service" \
  --uninstaller "/absolute/Uninstall MixRelay Driver.app" \
  --output "/absolute/empty/payload"
```

Preparation checks both architecture slices and bundle IDs but is not a PKG,
signature, install or runtime qualification. Then run
`scripts/build-macos-installer-pkgs.mjs` with explicit `--payload`,
`--installerCli`, `--payloadChecker`, `--teamId`, `--version`,
`--signingIdentity "Developer ID Installer: Publisher (TEAM_ID)"` and `--output`.
It verifies the Developer ID Application publisher and signed Universal 2 helpers
before producing one `MixRelay-Driver-Installer-VERSION.pkg` for installation and
update. It also installs `/Applications/Uninstall MixRelay Driver.app` and the
on-demand `io.mixrelay.driver.removal` system Mach service. The PKG has one fixed
installation component, no removal choices, no Installer plug-in, and no external
preflight program; external scripts are disabled in Distribution. Removal happens
in the dedicated app, with a default-off option to delete saved Driver settings.
Notarization and stapling remain required before publication. Install, update and removal reload CoreAudio to
load or unload the HAL; they do not require a Mac reboot. The localized Installer
readme describes the free virtual audio Driver and status-bar configuration,
warns that install/update briefly stops all system audio, and advises quitting
recording and call apps first. It states that no reboot is needed and saved
Driver settings are preserved on update. Runtime device creation and removal do
not reload CoreAudio.

CoreAudio termination must use the OS process-signal path, not `launchctl
kickstart -k`, which Apple blocks for critical services with SIP enabled
([macOS 14.4 release notes](https://developer.apple.com/documentation/macos-release-notes/macos-14_4-release-notes)).
Keep SIP enabled; successful termination alone is not proof that the HAL loaded.

Both artifacts retain CHANGELOG and original Driver terms; install/update shows
the selected-language Driver terms for agreement using Installer's native RTF resource
support ([Apple Distribution XML reference](https://developer.apple.com/library/archive/documentation/DeveloperTools/Reference/DistributionDefinitionRef/Chapters/Distribution_XML_Ref.html)).
Keep Installer's native Continue → Agree flow; do not add a custom agreement
dialog or redundant instructions. Installer owns its button and confirmation text.
The readme and agreement use 11-point Helvetica body text with 14-point line
spacing, a 14-point product title and a separate 12-point document subtitle.
Section headings and topic paragraphs remain distinct, and text wraps automatically
without fixed colors. The agreement is titled Software License Agreement;
community conduct rules are a separate document type.
Agreement presentation rejoins source column wraps and escapes RTF syntax and
Unicode; every legal word, business contact and agreement version remains.
Internal review notes and release checklists are kept in `docs/legal-review.md`,
outside the customer agreements and Installer resources.
The canonical plain `LICENSE-Driver.txt` ships byte-for-byte intact in the
installed Manager bundle, alongside complete Japanese, Korean,
Simplified Chinese, Traditional Chinese, French, German, Spanish, Portuguese,
Italian and Russian Driver agreements.
The Manager also includes complete App agreements in all eleven languages.
The required language/file mapping is `packaging/localization/languages.json`.
Installer displays `INSTALLER-LICENSE.rtf` from native `en.lproj`, `ja.lproj`,
`ko.lproj`, `zh-Hans.lproj`, `zh-Hant.lproj`, `fr.lproj`, `de.lproj`, `es.lproj`,
`pt.lproj`, `it.lproj` and `ru.lproj`
resources and owns the language selector. Keep these files only in the language
directories: unlocalized copies shadow the selected language in Installer.
Installation guidance is localized through the same directories.
The concise `INSTALLER-README.rtf` does not append
release notes; `CHANGELOG.md` remains separately bundled in the payload and unchanged.
The packaged Manager also retains App terms and bundled
third-party notices without treating Driver as a paid App purchase.

The native CLI is
`mixrelay-macos-installer install TEAM_ID MANAGER HAL DRIVER TEMPLATE UNINSTALLER REMOVAL_PLIST --reload-coreaudio`
or `remove TEAM_ID --reload-coreaudio [--delete-user-data]`. The explicit reload
flag is required before any install/remove mutation. The CLI requires an already authorized root
process and never elevates itself. It validates exact targets, signatures,
account ownership and the closed launchd definition. Updates require the graph
App, graph engine and Driver Manager to be closed. After administrator authorization
and source verification, the transaction observes the two apps in the original
installation user's session and requests acknowledged native Quit. It initializes
and acquires lifecycle leases only after Quit completes. Following a successful
update it reopens only the apps it actually closed, Driver first and graph App
second. A fresh install or previously stopped app is not automatically launched. Relaunch
success requires observing the exact bundle running under the same UID within
15 seconds after LaunchServices accepts the request.
A recoverable failure restores the prior running apps only after installed payload
verification; failed rollback never launches them. The PKG's native `must-close`
requirement applies only to the removal app, so Installer cannot erase the
Driver/graph running-state observation before the transaction begins. Older graph
App builds that treat native Quit as window hiding must first be explicitly quit
through their app/tray menu; the corrected managed native-Quit policy ships with
the graph App itself, which this Driver PKG never replaces. Their fixed lifecycle inodes
`app.owner`, `engine.owner` and `driver-manager.owner` remain intact. An exclusive
`installer.owner` serializes transactions. The Driver service holds none of those
process-owner locks.

Under retained locks, update/removal stops only the verified Driver job and
waits for its observed process to exit before replacing files. Installation then
registers the Driver listener, reloads the fixed CoreAudio service and checks the
actual Driver Ready state, installed version and launchd process identity.
Removal reloads CoreAudio after file removal and confirms that the HAL is absent. The fixed bundle-ID lookup runs
in a read-only child of the verified installer executable, with a 30-second parent
process deadline. A blocked CoreAudio API call is killed and reaped; it cannot
leave the transaction waiting on an uncancellable thread. Failed lookups, signals
and timeouts never count as successful removal. The same bounded check runs in
preflight before Driver file mutation, so an unresponsive audio server rejects the
operation before replacing/removing payloads. `mixrelay-macos-installer
check-coreaudio` runs that read-only preflight check without installation or
administrator authentication.
Failure drains the new Driver before restoring files and prior registration;
if activation was attempted, recovery also reloads the restored HAL. Incomplete
recovery is reported as a failure. Original files are temporary rollback backups,
not a permanent recovery archive. Every transaction cleans its private staging
under the retained lifecycle locks after success or after the rollback attempt,
even if rollback fails. Both rollback and cleanup errors remain visible; cleanup
failure never counts as success. The next transaction also removes abandoned
Driver staging from older versions, including its original backups. No other
service or Mac reboot is requested.

The removal app requests Authorization Services' named `io.mixrelay.driver.remove`
right. Its localized operation description is supplied by the installed app;
macOS controls the surrounding authentication dialog. The normal-user app never
requests general `system.privilege.admin` execution, invokes deprecated privileged
execution, or hands removal to Installer. The root-owned helper accepts only a
version, authorization external form and settings Bool over XPC. Both peers enforce
fixed Developer ID Application code identities and the same publisher. The helper
checks the already granted named right without interaction before touching Driver.
After authorization, the existing CLI asks the installed Manager to quit in its own
session, then removes the Manager, service, HAL, Driver launchd entry, removal app
and helper plist under the existing rollback guards. The original installation
account controls optional settings deletion. Manager caches are cleaned with user
privileges; the default-off settings option deletes only `drivers.json` and
`drivers.json.stopped`. Cleanup errors are failures, and the UI requires an explicit
committed success reply plus absent owned paths and helper registration. The helper
retires itself after that reply. Cancelling confirmation or authentication changes
no Driver state. Removal has no license-agreement step.
The listener first verifies and copies its fixed executable into a root-owned
private directory, then execs the same signed code without changing launchd's PID.
That executable remains on disk until the signed reply is accepted and the caller
acknowledges completion, so removing the installed bundle does not invalidate the
reply's code-signing check. The same verified copy serves the removal child and
normal-user cleanup actions. Completion cleans the private copy before retiring
the job; cleanup failure keeps registration and prevents the GUI success gate.
Idle rejection and normal SIGTERM clean the copy without unregistering the installed
job. A committed caller's disconnect also finalizes removal. The 330-second abandoned
completion bound is after the client's 300-second reply deadline. Actual XPC fixtures
cover deletion of sender/caller executables, signed success/ACK delivery, and native
listener SIGTERM cleanup in temporary user jobs without touching installed Driver.
End users need no Rust, Xcode, SDK, manual file copies or service configuration.

Source fixtures and package preparation are not clean-machine proof. Signed PKG
notarization, first install, HAL loading, manager/device readback, persistent
loopback after graph-app exit, explicit Driver-menu Quit stopping devices,
manager relaunch restoring settings, update/settings preservation and full removal/reinstall
must be exercised with the actual artifact.

The Driver manager's explicit Quit waits for native device removal and a saved
inactive run state before exiting. Saved device geometry and directional formats
remain in `drivers.json`; the regular `drivers.json.stopped` sibling marks the
stop without changing the settings JSON schema. Restarting the launchd service
does not reactivate them. The prior service release ignores this marker on
rollback and may start devices under its previous policy, but can still read the
unchanged device-settings schema. Selecting Delete saved settings removes both.
Reopening MixRelay Driver activates the retained inventory. The control service
and installed HAL stay available; this action does not uninstall the Driver.
Graph-app exit and window dismissal do not request this stop. An unsuccessful
stop or state save leaves the manager open and displays a localized error.

The optional prompt to move a completed installer to Trash belongs to macOS
Installer. On macOS 27.0.1 its native eligibility checks exclude packages outside
Downloads, non-writable packages and packages embedded in an app bundle. The
locally built artifact path is therefore not a test of that prompt. Release testing
must include a real browser download to Downloads. No privileged postinstall
script deletes or trashes the user's downloaded PKG.
