# Release qualification

`support/environments.json` defines supported qualification targets, CPU architectures,
dependency floors, build images, artifact mappings and required scenarios. Product policy
and unfinished product work remain in [product.md](product.md). The generated
[environment inventory](validation/supported-environments.md) lists the required targets;
[historical evidence](validation/platform-matrix.md) records earlier builds.

Run `pnpm support:check` after changing build or packaging configuration. When changing
the contract, run `pnpm support:sync` to regenerate Compose and the environment table.
The checker rejects different deployment and dependency minima in Tauri and PipeWire.

## Build once, qualify the final files

The `Native build and unit tests` workflow reads its matrices from the contract. Its
hosted macOS jobs cover Intel and Apple Silicon; Linux containers run on native x64
and arm64 hosts. These jobs check source code. They do not qualify installed audio.
GitHub documents the current [hosted runner labels and architectures](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).

The `Build Linux release candidates once per CPU` workflow builds one DEB for each
CPU on the contract's Debian baseline. Debian, Ubuntu and Kali qualification all use
that CPU's same DEB. Arch wraps the x64 DEB payload without recompiling or stripping
its executables. Keep `release-input.json` with the build evidence. RPM remains a
packaging capability; it has no qualified distribution target and cannot be included
in a release candidate under this contract.

Build the Universal macOS PKG from the same source commit with the existing signing
workflow: first `pnpm signing:check`, then `pnpm release:macos --output EMPTY_DIRECTORY`.
Use the notarized, stapled final PKG. Both CPUs and every macOS target install this
same file. Finish signing and stapling before calculating candidate hashes.
Use a checkout of the selected commit without local source changes; record the
Linux workflow run and macOS build result with the candidate. This procedure does
not infer a binary's source commit from its bytes.

Collect the four final artifacts in a fresh directory. For example:

```sh
pnpm release:candidate --directory /absolute/candidate \
  --source-revision FULL_40_CHARACTER_COMMIT_SHA --version 1.2.3 \
  --artifact macos-universal-pkg:MixRelay-1.2.3.pkg \
  --artifact linux-x64-deb:MixRelay-1.2.3-x64.deb \
  --artifact linux-arm64-deb:MixRelay-1.2.3-arm64.deb \
  --artifact linux-x64-arch:mix-relay-1.2.3-1-x86_64.pkg.tar.zst
```

This writes `mix-relay.candidate.json` with the source commit, contract SHA-256 and
each file's size and SHA-256. Rebuilding, signing, stapling or repacking changes the
candidate and requires another qualification run. Attach these files to a draft or
prerelease GitHub release; creating a candidate does not publish the product.

## Native runners

`Qualify exact release artifacts` is manually dispatched from `main` with the
candidate release tag. It accepts source commits already in `main`, checks the
successful source build/unit-test workflow for that exact `main` commit, contract
and file hashes, and fails before scheduling if any target lacks an online
runner. Its preflight uses `QUALIFICATION_RUNNER_READ_TOKEN`, a fine-grained token
with repository Administration read permission. The ordinary workflow token cannot
read that administrative runner inventory. The token is used only on the hosted
preflight job.

Provision resettable desktop QA sessions with the labels in the contract. Register
each Actions runner for one job and reset its VM or dedicated QA installation after
the job, including after failure. These machines need a normal desktop user, native
audio services, installation tools, noninteractive `sudo` and the OS permissions used
by the probe. macOS needs Xcode command-line tools; Linux needs `pw-cat`, `pipewire`,
`wireplumber`, `bsdtar` for Arch, and its package manager. Never register a personal
workstation. No pull-request event may reach these privileged runners.

The installed runner rejects containers, Rosetta, another CPU/OS/distro, dependencies
below the contract floors and pre-existing installed MixRelay executables. It opens
the normal installed app and normal control sockets. It compares installed core
executables against the candidate package, exercises graph save/reopen, emits native
stereo PCM through the free installation's default device, and checks the recorder's
finalized RF64 sizes and samples. It restores its graph and stops its app; runner reset
removes the installation, default device and remaining QA data. Cleanup failure also
fails qualification.

For diagnosis on a disposable QA session:

```sh
pnpm test:e2e:installed --target debian-13-x64 \
  --directory /absolute/candidate \
  --candidate /absolute/candidate/mix-relay.candidate.json \
  --output /absolute/new-evidence --disposable-environment
```

Local diagnostic reports have no trusted CI provenance and cannot authorize publication.
The API scenarios do not check rendered UI, launch-icon behaviour, Free-state display
or permission-dialog recovery. `GRAPH-01` (installed UI) and macOS `AUDIO-11` (free
whole-system capture) currently produce blocked results because their acceptance
implementations are absent. These required results prevent the workflow from passing.
The larger scenario catalog in the historical inventory still needs coverage; this
seven-scenario baseline does not establish the other product requirements as verified.

## Inventory and publication

The gate produces a per-environment inventory with the candidate artifact hash and
per-scenario passed, failed, blocked or unexecuted results. Before selecting a candidate,
run `pnpm validation:inventory` to see all targets as unexecuted. It hashes raw evidence
again and rejects missing, skipped,
duplicate or mismatched reports, another source/contract, mixed workflow runs and
failed cleanup. A rolling distribution records its actual OS release, kernel,
dependency versions and package-list hash on every run. Version rules in the contract
are qualification representatives: they do not mean every OS patch was executed.

```sh
pnpm validation:inventory --directory /absolute/candidate \
  --candidate /absolute/candidate/mix-relay.candidate.json \
  --reports /absolute/qualification-evidence
pnpm release:qualify --directory /absolute/candidate \
  --candidate /absolute/candidate/mix-relay.candidate.json \
  --reports /absolute/qualification-evidence
```

A successful native workflow creates `publication/mix-relay.release.json` and
`publication/mix-relay.qualification.json` inside its `qualification-evidence` Actions
artifact. Attach both files, the candidate JSON and the unchanged installers to the
final `vMAJOR.MINOR.PATCH` GitHub release. Keep the Actions evidence for the publication
window (currently 90 days). After expiration, rerun qualification before publication.

`publish-updates.yml` checks the final tag's source commit, candidate hashes, manifest
hash, successful source CI and the successful `main` qualification workflow run and
attempt. It downloads
that run's raw evidence directly from Actions and reruns the gate before requesting
central catalog publication. Edited release attachments or an arbitrary report JSON
cannot substitute for that successful workflow. The central publisher retains its
existing manifest schema. The app reads Linux distribution identity and selects the
package format from the embedded support contract: DEB for Debian/Ubuntu/Kali and
Arch for Arch x64. It filters the catalog's OS/CPU results locally, including when
revalidating a download link. It does not request the server's older format filter.

The workflow guards catalog dispatch, not the GitHub release button. Use draft and
prerelease assets until qualification passes. No workflow here automatically publishes
a GitHub release or changes repository visibility.
