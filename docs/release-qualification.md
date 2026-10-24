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

Linux 링크는 keyring의 정적 `libdbus.a` 심볼을 외부에 노출하지 않는다. GTK/AT-SPI가
시스템 libdbus로 만든 객체를 private libdbus가 처리하는 충돌을 방지한다.
이 링크 설정을 바꾼 경우 Linux 패키지를 다시 빌드해야 하며, 기존 파일로 실행한
suite 결과는 수정한 실행 파일의 검증이 아니다.

Build the Universal macOS PKG from the same source commit with the existing signing
workflow: first `pnpm signing:check`, then `pnpm release:macos --output EMPTY_DIRECTORY`.
Use the notarized, stapled final PKG. Installed verification requires `stapler validate`
to succeed. Only its observed exit 68 with `NSURLErrorDomain Code=-1005` permits a retry:
three attempts with 30- and 60-second waits, recorded in `package-verification.json`.
Invalid tickets and exhausted transport failures stop before extraction or installation.
On hosted macOS 15, validation uses HTTPS over TCP to avoid the
[confirmed HTTP/3 runner failure](https://github.com/actions/runner-images/issues/14924).
The QA adapter adds a private rule below the existing platform packet-filter anchor,
blocks outbound UDP 443 only while validating, then removes that rule and releases its
own enable reference before extraction. It preserves the main ruleset and records all
commands and cleanup in `apple-ticket-transport.json`. Setup or cleanup failure fails the
case; signature, Gatekeeper and Apple ticket verification remain required.
Both CPUs and every macOS target install this
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
설치 완료 결과, 새 코드의 서비스 등록·앱 재실행에는 역할별 entitlement allowlist의
정확한 일치를 요구한다. 실패·롤백 후 이전 코드를 다시 활성화할 때는 소유권과 작업 전
파일·launchd 스냅샷의 일치를 확인하며 현재 entitlement 집합을 요구하지 않는다.
앱·엔진·HAL·Driver·제거 앱·내장 설치 도우미가 모두 대상이다.
업데이트·제거 검증에는 entitlement가 없거나 현재 allowlist와 다른 기존 자사 설치본을
포함한다. 실패한 업그레이드에서 이전 코드의 복원·재활성화, 다른 팀·미서명 코드의 거부와
새 payload·설치 완료 결과의 엄격한 검증도 확인한다.
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
the report records the observed version. The macOS deployment minimum remains 14.2
because the process-tap API requires it. Qualification uses the available patch
of each hosted major-version representative, rather than requiring an exact 14.2
image. macOS 14 Intel and patches below the observed hosted versions remain
untested; the support minimum does not claim those exact combinations were tested.

Linux uses `ubuntu-24.04` or `ubuntu-24.04-arm` as the VM host. QEMU boots each
distribution's complete cloud image with its own kernel, systemd, X11 display,
PipeWire and WirePlumber user services. The contract pins official Debian, Ubuntu,
Kali and Arch image URLs and checksums. The adapter checks the download before boot,
uses a fresh disk overlay and SSH key, forwards only public workflow identity, then
collects evidence and deletes the VM. KVM is used when available; otherwise the same
guest CPU architecture runs with TCG. `vm-session.json` records the accelerator;
ARM TCG uses the Cortex-A72 model. TCG guests use up to four of the host's allocated
CPUs, while KVM guests use up to two; neither allocation exceeds the available host
count. `vm-session.json` records `hostCpuCount` and `guestCpuCount`. TCG results are
functional evidence and do not establish hardware latency.

폐기할 guest는 고정된 커널을 그대로 사용하므로 cloud-init에서 needrestart의
커널 이미지 검사만 끈다(`kernelhints = 0`). Ubuntu ARM에서 제품 설정을 마친 뒤에도
커널 검사에 머물러 전체 설치의 20분 제한을 넘긴 증거에 따른 설정이며, 라이브러리·서비스 검사와 패키지 설치·
실행 파일 대조는 유지한다. 설치 제한 시간은 늘리지 않으며 이 설정은
`vm-session.json`의 `provisioning.needrestartKernelHints`에 기록한다.

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
finalized RF64 sizes and samples. It restores its graph, stops its app and explicitly
removes the installed product.
The same session then runs baseline-to-candidate upgrade qualification and removal.
Disposal removes the remaining QA data. Cleanup failure also fails qualification.
Linux shutdown sends one SIGTERM to each owned editor/controller and observes actual
exit for up to 150 seconds, the same budget as the macOS Quit command. The controller
drains accepted work and waits for audio finalization before exiting; this is not a
five-second contract. `shutdown-observation.json` records elapsed time and remaining
processes. A remaining process fails cleanup; the observer does not force-kill it.

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
`GRAPH-01` uses the installed editor to create and connect nodes and checks the committed
graph against the engine. Linux submenu navigation refreshes native accessibility state
and requires an observed expansion before selecting a leaf. Each submenu permits at most
three focus requests and three Right Arrow submissions; each leaf receives one pointer click.
An unexpanded menu or an uncommitted graph fails the case.

Both hosted workflows build and sign one XCTest observer before
installing the product. Preparation grants Accessibility and Screen Capture to that exact code requirement,
restarts the disposable runner's system TCC daemon and confirms its replacement PID.
The observer records and requires its actual Accessibility and Screen Capture access before
interacting with the installed editor. Product Microphone and Audio Capture consent still
uses the first-use dialogs. Fresh and upgrade reuse the prepared observer; each XCTest
configuration keeps its original
product paths and changes only `MIXRELAY_UI_EVIDENCE`. The installed phase verifies the
signed code and uses `test-without-building` with signing disabled. The synthetic native
probe self-test also compiles before installation. The audio scenarios retain their first
native I/O and full PCM checks.

The GUI measurement probe completes its own microphone authorization before preparing any
device or starting I/O. This follows [Apple's capture authorization procedure](https://developer.apple.com/documentation/avfoundation/requesting-authorization-to-capture-and-save-media).
Denied, restricted or unresolved authorization fails measurement. A nongraphical CLI keeps
CoreAudio's existing broker path; installed E2E requires the GUI's authorized status. The probe
records its initial and final status and whether it requested consent in `inputAuthorization`.
MixRelay's first-use microphone and system-audio dialogs continue through the UI observer.
The signal still starts with its original first frame and runs for the complete requested
duration; the probe performs no preliminary audio run.

The finite native measurement holds a process activity with
`NSActivityUserInitiatedAllowingIdleSystemSleep | NSActivityLatencyCritical` while its
I/O procedures run. The observer verifies `activityProtection` in the probe result.
The [activity API](https://developer.apple.com/documentation/foundation/processinfo/beginactivity(options:reason:))
classifies the background GUI probe's recording work; the token ends when the I/O
routine returns, including failed measurements. It does not guarantee a virtual host's scheduling.
Failed PCM also collects bounded host CPU and process snapshots after the signal ends.
Collection preserves the PCM failure and never repeats the signal.

For a hosted diagnostic job, prepare these tools before invoking the installed runner:

```sh
node scripts/e2e/prepare-macos-host.mjs macos-host
node scripts/e2e/prepare-macos-ui.mjs
bash scripts/e2e/macos-ui.sh --prepare installed-ui-tools
zsh scripts/probe-macos-roundtrip.sh --self-test
export MIXRELAY_E2E_MACOS_UI_TOOLS="$PWD/installed-ui-tools"
```

The signed observer's identity and TCC restart PIDs are included in the target artifact.
The macOS UI observer handles first-use audio consent and
remains active through Quit and removal. macOS `AUDIO-11` exercises free whole-system
capture. After recording the UI screenshot, accessibility tree and committed graph,
macOS minimizes the editor window and measures background native audio at 48kHz/128 frames;
`installed-ui-audio-phase.json` records the editor state. The consent observer remains
active. The system-capture graph is prepared before its two owned tone apps start.
Recording starts after both channels produce live graph meters within 30 seconds.
`whole-system-ready.json` preserves those observations, including a readiness failure.
Their failures and missing evidence prevent qualification. API results alone
do not establish rendered UI, launch-icon behaviour, Free-state display or permission
recovery. The [950ff3ac hosted failure investigation](validation/macos-hosted-950ff3ac.md)
records the observed PID selection, first-consent and Quit failures and the remaining
installed validation.
The larger scenario catalog in the historical inventory still needs coverage; this
scenario baseline does not establish the other product requirements as verified.

## 이전 후보에서 업그레이드 검증

`candidate-e2e.yml`과 `qualify-release.yml`은 기존 후보 입력 외에 다음 두 입력을 필수로 받는다.

- `baseline_macos_candidate_tag`: 이전 PKG와 전체 소스 SHA를 담은 `source-revision.txt`가 있는 draft 또는 prerelease 태그.
- `baseline_linux_build_run`: 같은 소스의 `Build Linux release candidates once per CPU` 성공 실행 ID.

2026-10-24 조회 기준 `qa-install-8582d15`와 `qa-install-03652c5`에는 각각 macOS
`MixRelay-Installer-0.1.0.pkg`와 소스 기록이 있다. Linux 파일은 이 draft에 없으므로 별도
빌드 실행에서 가져온다. 서명 정책 변경 전 후보 `qa-install-8582d15`의 Linux 빌드 실행은
`39939576577`, `qa-install-03652c5`의 실행은 `39979549280`이다.
각 성공 실행의 `candidate-linux-x64-deb`, `candidate-linux-arm64-deb`,
`candidate-linux-x64-arch` 아티팩트를 사용한다. Arch 파일은
`mix-relay-0.1.0-1-x86_64.pkg.tar.zst` 형태다. 이 값들은 실행 예시이며 기본값으로 고정하지 않는다.

진단 워크플로의 예시 입력은 후보 태그 `qa-install-03652c5`, 후보 Linux 실행
`39979549280`, baseline 태그 `qa-install-8582d15`, baseline Linux 실행 `39939576577`이다.
워크플로 화면에서 해당 입력을 지정하고 `targets`를 비우면 hosted 대상 전체를 실행한다.
출시 검증에는 `main`에 포함된 후보와 성공한 소스 CI 등 기존 조건도 충족해야 한다.
설치 파일은 선택한 후보의 원본이며, 실행 스크립트는 dispatch한 브랜치의 suite를 사용한다.
후보 소스와 suite 소스는 리포트에 별도로 기록한다.

선택기는 baseline의 저장소·빌드 워크플로·성공 여부·macOS/Linux 소스 일치와 후보의 선행
커밋인지 확인한다. 파일별 크기·SHA-256·소스·버전·빌드 실행 ID를
`candidate/baseline/baseline.json`에 보존한다. 같은 소스, 같은 설치 파일, 더 높은 제품 버전,
분기된 커밋은 거부한다. 버전 번호가 같은 개발 후보는 소스와 파일이 다르면 시험할 수 있다.
일부 baseline 패키지 형식이 없으면 준비 단계의 matrix와 Actions 요약에 해당 대상의
`MISSING baseline artifact`를 표시한다. 대상 job은 생략하지 않으며 명시적으로 실패한다.

대상마다 runner 또는 Linux VM 하나를 재사용해 다음 순서로 실행한다.

1. 후보를 새로 설치하고 기존 PKG·앱·UI·그래프·오디오·녹음 검사를 실행한 뒤 제거한다.
2. fresh 제거가 끝나면 QA 사용자 소유의 앱·엔진·Driver 상태를 `upgrade/fresh-state/`로 옮겨
   baseline이 새 상태에서 시작하게 한다. 외부 프로젝트·녹음 파일은 그대로 둔다.
   baseline을 정상 설치 경로로 설치하고 앱과 Driver를 실행한다. 무료 Driver의 실제
   `/api/v1/driver` PUT 경로로 양방향 채널 수와 PCM 형식을 기본값과 다르게 저장한다.
   대체 샘플레이트를 지원하면 그것도 변경한다. 현재 레이트만 지원하는 Linux에서는
   그 레이트를 유지한다. 명령마다 revision을 확인하고 양방향 현재 형식의 readback과
   저장 오류 부재를 확인한다. 장치 추가·이름 변경에는 유료 인증이 필요하므로 사용하지 않는다.
   게인 -6dB·녹음기 연결 그래프를 실제 workspace API로 `.mix` 파일에 저장하고,
   저장 결과의 활성 그래프 ID·노드·설정·연결이 작성한 내용과 같은지 확인한다.
   생략한 label과 control은 그래프 문서의 기본값(null, enabled=true, muted=false)으로
   비교하며, 기본값과 다른 제어 상태나 설정·연결 손실은 실패한다. baseline 그래프의
   native 오디오 활성화·수락은 요구하지 않으며, 후보를 덮어 설치한 뒤 기존 검사로 확인한다.
3. 앱을 정상 종료한 뒤 macOS는 `installer -pkg`, Linux는 `apt-get install --reinstall` 또는
   `pacman -U`로 후보를 덮어 설치한다. 이전 제품을 제거하는 명령은 이 두 설치 사이에 없다.
4. 설치 실행 파일을 후보 패키지의 해시와 대조하고 후보 버전의 Driver·엔진 준비 상태,
   복구된 그래프·장치 설정·저장 파일 해시를 확인한다. Driver는 장치 spec뿐 아니라
   관측 샘플레이트와 양방향 현재 형식의 샘플레이트·채널 수·PCM 표현까지 비교한다.
   저장 파일도 다시 열어 비교한 뒤 Driver의 원래 기본 설정과 현재 형식을 복원하고
   그래프를 비워 기존 후보 검사를 모두 다시 실행한다. fresh 검사는 이 복원 경로를 거치지 않는다.
5. 후보를 제거하고 설치 파일의 부재와 외부 `.mix`·녹음 파일 보존을 확인한다.
   macOS는 설치된 서명 도우미의 private copy로 native 제거 트랜잭션을 실행하며,
   Linux는 배포판 패키지 관리자로 제거한다. 제거 GUI의 인증 대화상자 검증은 포함하지 않는다.

`fresh/`와 `upgrade/`에는 각 모드의 설치·종료·제거 로그와 원시 증거가 남는다.
최상위 대상 리포트는 fresh 결과와 업그레이드 후 전체 검사 결과를 묶는다.
필수 시나리오 `UPGRADE-01`은 baseline 설치·상태 보존·업그레이드 후 검사·제거 중 하나라도
실패하면 실패하며, `PKG-02`는 fresh 제거 결과를 기록한다. 업그레이드 리포트와 원시 증거도
gate의 해시 검사 대상이다. 기존 실패를 성공으로 바꾸거나 미실행 대상을 생략하지 않는다.

이 검사는 기존 앱의 서명 정책을 새 설치기가 거부하는 회귀, 같은 버전의 오래된 실행 파일 잔류,
저장 그래프·장치 설정 손실과 업그레이드 후 실행·오디오 회귀를 검출한다. 보존 검사의 범위는
단일 그래프의 ID·노드·설정·연결, 외부 `.mix` 파일, 무료 Driver의 장치 구성과 현재 형식이다.
앱 환경설정·레이아웃·여러 그래프 선택·유료 인증·장시간 녹음의 보존은 검증하지 않는다.
실제 설치·업그레이드 성공은
해당 후보로 hosted CI를 실행한 뒤에만 판단한다. 최소 지원 버전 14.2와 실제 검증한
macOS 패치·CPU 조합은 구별해 기록한다. Arch arm64와 macOS 27 x64는 지원 계약의 대상이 아니다.

로컬 진단도 초기화 가능한 QA 세션에서만 실행한다.

```sh
pnpm test:e2e:installed --target debian-13-x64 \
  --directory /absolute/candidate \
  --candidate /absolute/candidate/mix-relay.candidate.json \
  --baseline /absolute/candidate/baseline \
  --output /absolute/new-evidence --disposable-environment
```

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
