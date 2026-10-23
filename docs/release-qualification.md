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
same file on the CPUs supported by each macOS release. macOS 27 has only an arm64
target because [Apple's compatibility list](https://support.apple.com/en-us/127255)
contains Apple silicon models; Intel targets end at macOS 26. Finish signing and
stapling before calculating candidate hashes.
Use a checkout of the selected commit without local source changes; record the
Linux workflow run and macOS build result with the candidate. This procedure does
not infer a binary's source commit from its bytes.

macOS 설치·업데이트·제거 전에는 기존 코드의 번들 ID·실행 파일, 게시자 팀의 Developer ID
요구사항, 유효한 서명, Universal 2, Hardened Runtime·보안 타임스탬프로 소유권을 확인한다.
기존 코드의 entitlement 집합은 소유권 판단에 사용하지 않는다. 새 원본·비공개 payload와
설치 완료 결과, 서비스 등록·앱 재실행·롤백 후 재실행에는 역할별 entitlement allowlist의
정확한 일치를 요구한다. 앱·엔진·HAL·Driver·제거 앱·내장 설치 도우미가 모두 대상이다.
업데이트·제거 검증에는 entitlement가 없거나 현재 allowlist와 다른 기존 자사 설치본을
포함한다. 다른 팀·미서명 코드의 거부와 새 payload·설치 완료 결과의 엄격한 검증도 확인한다.
소스의 주입된 서명 메타데이터 테스트와 실제 서명 PKG의 설치 검증 결과는 구분해 기록한다.

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

## GitHub-hosted execution

Before merging a source branch, use `Test installed candidate on hosted environments`
to diagnose the same final installers on the hosted matrix. Build Linux packages with
`Build Linux release candidates once per CPU`, then attach the signed, notarized macOS
PKG and a `source-revision.txt` containing its full source SHA to a draft release.
Dispatch `candidate-e2e.yml` on the source branch with `linux_build_run` and
`macos_candidate_tag`. The selector checks the Linux workflow identity, successful build,
matching source records and all four artifact hashes. The suite revision is recorded
separately, so test-tool fixes can exercise existing installers without rebuilding them.
The resulting `installed-e2e-evidence` artifact contains the exact candidate, raw
per-environment evidence and `inventory.md`, including failed and unexecuted scenarios.
This diagnostic workflow runs the full gate but cannot create a publication receipt.
The optional `targets` input accepts comma-separated hosted target IDs for diagnosis.
Unselected targets remain unexecuted in the full gate. Use all targets for release
qualification.

Only preparation jobs receive `contents: write`: GitHub exposes draft releases to
callers with [push access](https://docs.github.com/en/rest/releases/releases#list-releases).
Installation jobs keep read-only workflow permissions, and no workflow token enters
the Linux guest.

`Qualify exact release artifacts` is manually dispatched from `main` with the
candidate release tag. It accepts source commits already in `main`, checks the
successful source build/unit-test workflow for that exact `main` commit, contract
and file hashes. It uses standard GitHub-hosted runners and the ordinary workflow
token. It requires no registered QA machines or repository administration token.

macOS installs the Universal PKG directly on six hosted OS/CPU combinations: 14 arm64,
15 arm64/x64, 26 arm64/x64 and 27 arm64. GitHub chooses each image's patch version;
the report records the observed version. macOS 14.2 on both CPUs remains a required
coverage gap: GitHub cannot select that patch, and has no macOS 14 Intel runner.
Those targets stay in the inventory as unexecuted and prevent release qualification.
The macOS 14 arm64 hosted job does not substitute for a 14.2 result.

Linux uses `ubuntu-24.04` or `ubuntu-24.04-arm` as the VM host. QEMU boots each
distribution's complete cloud image with its own kernel, systemd, X11 display,
PipeWire and WirePlumber user services. The contract pins official Debian, Ubuntu,
Kali and Arch image URLs and checksums. The adapter checks the download before boot,
uses a fresh disk overlay and SSH key, forwards only public workflow identity, then
collects evidence and deletes the VM. KVM is used when available; otherwise the same
guest CPU architecture runs with TCG. `vm-session.json` records the accelerator;
ARM TCG uses the Cortex-A72 model. TCG results are functional evidence and do not
establish hardware latency.
The pinned Kali 2026.2 ARM image contains a stale EFI mount PARTUUID. Cloud-init
preserves the original fstab and corrects that entry from the guest's EFI partition
in its disposable overlay; `vm-session.json` records this provisioning repair.

`Qualification VM readiness` boots all seven Linux combinations on pull requests
that change the VM tooling. It checks OS/CPU/dependency identity and desktop audio
prerequisites. Its `host-readiness` output does not qualify a release artifact.
The manual release workflow installs the candidate and runs the shared suite in
those same guests. Cloud-image downloads that expire or change fail verification;
update the contract to a new official image and checksum before the next run.

The installed runner rejects containers, Rosetta, another CPU/OS/distro, dependencies
below the contract floors and pre-existing installed MixRelay executables. It opens
the normal installed app and normal control sockets. It compares installed core
executables against the candidate package, exercises graph save/reopen, emits native
stereo PCM through the free installation's default device, and checks the recorder's
finalized RF64 sizes and samples. It restores its graph and stops its app; disposal
of the hosted job or guest removes the installation and remaining QA data. Cleanup failure also
fails qualification.

For diagnosis on a disposable session:

```sh
pnpm test:e2e:installed --target debian-13-x64 \
  --directory /absolute/candidate \
  --candidate /absolute/candidate/mix-relay.candidate.json \
  --output /absolute/new-evidence --disposable-environment
```

Local diagnostic reports have no trusted CI provenance and cannot authorize publication.
When the engine's readiness telemetry disagrees with the Driver, `APP-01` remains
failed. Independent native checks proceed only if the installed Driver API reports
ready at the candidate version and the engine reports the expected protocol, enabled
power and native backend. `readiness-disagreement.json` preserves both responses.
The gate still requires `APP-01` to pass.
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
All reports must belong to one workflow attempt. Use **Re-run all jobs** when retrying
qualification so successful jobs also produce evidence for the new attempt.

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
