# Coordinated HAL hot update

The coordinated installer captures whether the verified HAL bundle existed and
the loaded hosts' PID, C-locale `ps lstart`, and exact command name before
replacing files, under the installer lifecycle lease. An upgrade first
tries to restart only the loaded MixRelay HAL hosts. Fresh installs use the full
CoreAudio reload so the server discovers the new plug-in. Removal continues to
use the full reload and the existing HAL-absence check.

`src/native/hal_reload.rs` discovers PIDs with `/bin/ps -axo pid=,comm=` and accepts
only the exact `Core Audio Driver (MixRelayHAL.driver)` command name. Every
candidate in that snapshot must also have an exact `/usr/sbin/lsof -p PID -Fn`
path record for
`/Library/Audio/Plug-Ins/HAL/MixRelayHAL.driver/Contents/MacOS/MixRelayHAL`.
The transaction's existing payload verification establishes bundle identity
`io.mixrelay.audio.driver` and signature trust; process names alone authorize no
signal. After commit, a captured host may map either that original path or the
exact HAL backup path supplied by this transaction's filesystem layout: Darwin
reports the old vnode's current location after the bundle rename. Other backup
locations, deleted-file suffixes, diagnostics, and failed queries are rejected.
Hosts appearing only after capture are never signalled; a changed host set
selects the full reload fallback.

All captured identities and mappings are revalidated before any signal.
Immediately before each
`/bin/kill -TERM PID` or `/bin/kill -KILL PID`, the installer rechecks command name,
executable mapping, and the initially observed C-locale `ps lstart` timestamp.
After TERM it polls for PID absence for three seconds; if still present it
revalidates before KILL and polls for absence again. Process queries and signals
use the existing bounded command runner with absolute paths, a cleared
environment, and no shell. The three-second polling budget is separate from the
runner's ten-second deadline on each child command.

No candidate, any validation/command failure, or unconfirmed termination selects
the existing `/usr/bin/killall -KILL -u _coreaudiod coreaudiod` fallback. If the
existing Driver readiness wait fails after targeted restart, the installer runs
that full reload and repeats the same readiness wait. A final failure remains a
transaction failure and follows the existing drain, file rollback, service
restoration, and cleanup ordering. Restoring a previous HAL captures hosts again
before file rollback, then prefers targeted restart with the same exact-path and
identity checks. Rollback removes the replacement HAL and renames the saved HAL
back to its original path. A host whose removed vnode has no verifiable exact
mapping selects full reload. When no Driver was registered, recovery readiness
requires the restored HAL plug-in to be loaded; an answering CoreAudio server
with no plug-in is insufficient and triggers full fallback after targeted restart.
If no reload was attempted, restoration retains the previous
bootstrap-and-readiness behavior.

The existing explicit `--reload-coreaudio` approval is required before mutation,
including upgrades, because full fallback interrupts system audio. Installer logs
report `TargetedHost` or `FullCoreAudio`, and the reason for falling back. A
targeted restart interrupts MixRelay's own audio; installed-product testing is
still needed to verify other applications' audio remains continuous.

PID observations and signal delivery are separate OS operations, as in the
prototype; the timestamp adds generation checking but is only second-resolution
and cannot provide an atomic process handle.

Run development checks without root or live audio reloads:

```sh
cargo fmt -p mixrelay-macos-installer -- --check
node scripts/lab/native-check.mjs cargo clippy -p mixrelay-macos-installer --all-targets --all-features -- -D warnings
node scripts/lab/native-check.mjs cargo test -p mixrelay-macos-installer --lib
```

Unit tests inject process, command, and clock fakes for discovery, identity,
TERM/KILL escalation, PID reuse, exit confirmation, readiness fallback, and
restored-service activation. Transaction fixtures exercise actual file commit and
rollback with simulated vnode paths, including exact backup-path acceptance,
unrelated-path rejection, new hosts after commit, and absent-plug-in recovery.
They never deliver a live host signal or reload CoreAudio.
