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

The service uses the OS-verified installation user's HOME, short name and primary
group. Its fixed Mach service is `io.mixrelay.audio.driver`; RunAtLoad and KeepAlive
are true, so closing the manager does not remove devices or basic loopback.
Driver settings live in that user's
`Library/Application Support/io.mixrelay.driver/drivers.json`.
Updates and removal preserve the original installation account. After complete
removal, a fresh installation can bind the idle process leases to its new account
without replacing their inodes or changing the previous user's data. Switching
accounts while the Driver is installed does not transfer its settings or ownership.

Build the actual Universal 2 Driver service and manager with the Driver product
build. The service preparer is `scripts/prepare-driver-package.mjs`; the manager
build is `pnpm desktop:build --driver`, which selects the checked universal
builder's Driver profile. It also builds the removal GUI, installer CLI and
payload checker from source for both architectures, joins and signs their
Universal 2 outputs, and embeds `Uninstall MixRelay Driver.app` in the Manager's
`Contents/Helpers` before the final outer signature. Release inputs require
Developer ID Application signing; there is no unsigned release fallback.
Stage prebuilt artifacts with:

```sh
node scripts/prepare-macos-release-payload.mjs \
  --manager "/absolute/MixRelayDriver.app" \
  --hal "/absolute/MixRelayHAL.driver" \
  --driver "/absolute/mixrelay-driver-service" \
  --output "/absolute/empty/payload"
```

Preparation checks both architecture slices and bundle IDs but is not a PKG,
signature, install or runtime qualification. Then run
`scripts/build-macos-installer-pkgs.mjs` with explicit `--payload`,
`--installerCli`, `--payloadChecker`, `--teamId`, `--version`,
`--signingIdentity "Developer ID Installer: Publisher (TEAM_ID)"` and `--output`.
It verifies the Developer ID Application publisher and signed Universal 2 helpers
before producing `MixRelay-Driver-Installer-VERSION.pkg` and an intact standalone
copy of `Uninstall MixRelay Driver.app`. Users can download the distribution again
and run that removal app. It is also installed inside
`/Applications/MixRelay Driver.app/Contents/Helpers`; removal is not a status-bar
menu action. No uninstall PKG is produced. Notarization and stapling remain
required before publication. Install, update and removal reload CoreAudio to
load or unload the HAL; they do not require a Mac reboot. The English Installer
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
the English Driver terms for agreement using Installer's native RTF resource
support ([Apple Distribution XML reference](https://developer.apple.com/library/archive/documentation/DeveloperTools/Reference/DistributionDefinitionRef/Chapters/Distribution_XML_Ref.html)).
Keep Installer's native Continue → Agree flow; do not add a custom agreement
dialog or redundant instructions. Installer owns its button and confirmation text.
The readme and agreement use 12-point Helvetica body text, distinct title and
section headings, paragraph spacing and automatic wrapping without fixed colors.
Agreement presentation rejoins source column wraps and escapes RTF syntax and
Unicode; every legal word, business contact and distribution-draft notice remains.
The canonical plain `LICENSE-Driver.txt` ships byte-for-byte intact in the
installed Manager and removal app bundles. Installer displays
`INSTALLER-LICENSE.rtf`. The concise English `INSTALLER-README.rtf` does not append
release notes; `CHANGELOG.md` remains separately bundled in the payload and unchanged.
The removal app retains those terms
without an agreement step. The packaged Manager also retains App terms and bundled
third-party notices without treating Driver as a paid App purchase.

The native CLI is
`mixrelay-macos-installer install TEAM_ID MANAGER HAL DRIVER TEMPLATE --reload-coreaudio`
or `remove TEAM_ID --reload-coreaudio [--delete-user-data]`. The explicit reload
flag is required before any install/remove mutation. The CLI requires an already authorized root
process and never elevates itself. It validates exact targets, signatures,
account ownership and the closed launchd definition. Updates require the graph
App, graph engine and Driver Manager to be closed. The PKG declares both GUI
bundle IDs through Installer's native `must-close` requirement, so its normal
close/cancel confirmation runs before installation. Their fixed lifecycle inodes
`app.owner`, `engine.owner` and `driver-manager.owner` remain intact. An exclusive
`installer.owner` serializes transactions. The Driver service holds none of those
process-owner locks.

Under retained locks, update/removal stops only the verified Driver job and
waits for its observed process to exit before replacing files. Installation then
registers the Driver listener, reloads the fixed CoreAudio service and checks the
actual Driver Ready state, installed version and launchd process identity.
Removal reloads CoreAudio after file removal and confirms that the HAL is absent.
Failure drains the new Driver before restoring files and prior registration;
if activation was attempted, recovery also reloads the restored HAL. Incomplete
recovery retains private staging. No other service or Mac reboot is requested.

The removal app offers an initially unchecked **Delete saved Driver device settings**
choice and requests administrator authorization for the existing signed CLI.
Its standalone copy works independently of the Manager bundle being removed.
It removes the Driver Manager's caches, WebKit data, saved window state and preferences
with user privileges, and optionally the verified installation account's `drivers.json`.
The installed Manager, nested removal app, service, HAL and launchd entry are removed.
App graphs, caches, recordings and license authentication remain. Cancellation or failure
does not claim success; authorization cancellation reopens a previously running Manager.
End users need no Rust, Xcode, SDK, manual file copies or service configuration.

Source fixtures and package preparation are not clean-machine proof. Signed PKG
notarization, first install, HAL loading, manager/device readback, persistent
loopback after GUI exit, update/settings preservation and full removal/reinstall
must be exercised with the actual artifact.
