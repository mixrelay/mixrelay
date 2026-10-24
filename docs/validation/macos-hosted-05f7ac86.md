# 05f7ac86 macOS hosted E2E 실패 분석

실행 `40034216825`, suite·후보 소스 `05f7ac86`, workflow
`.github/workflows/candidate-e2e.yml`의 여섯 macOS 대상을 조사했다.
원본 증거 루트는 `/tmp/mr-e2e-r3/art/`, job 로그는 `/tmp/mr-e2e-r3/logs/`다.

## AUDIO-01: 제품의 stream usage 재알림

여섯 대상 모두 `installed-target-<target>/fresh/live-coreaudio-probe.txt`에
MixRelay IO의 입출력이 있다. 최상위 `coreaudio-probe.txt`는 제거 후 수집된 결과다.
fresh에서 HAL이 로드되거나 장치를 게시하지 못했다는 해석은 원본 증거와 다르다.

`installed-target-macos-26-arm64/fresh/live-coreaudio-system.log:1212`에는
coreaudiod PID 2168이 MixRelayHAL.driver를 로드하고 PID 2173의 원격 호스트를 만든
기록이 있다. 같은 파일 1233행에는 endpoint
`io.mixrelay.audio.endpoint.17e59d7342c14f11a8b46c2d913357e0`의 활성화가 기록돼 있다.
후보 PKG를 `/tmp/mr-e2e-r3/candidate-expanded/`에 추출해 확인한 HAL은
`Contents/Info.plist`, `Contents/MacOS/MixRelayHAL`, 서명 리소스를 갖춘 번들이다.
`codesign --verify --strict`를 통과했고 x86_64·arm64, runtime 서명, 보안 타임스탬프와
factory UUID `1E74B82D-D4E7-45C3-8DF0-9FEE0A3092AE`를 확인했다.
로컬 조회 원시는 `/tmp/mr-e2e-r3/candidate-hal-metadata.txt`에 있다.

여섯 대상의 `fresh/live-engine.sample.txt` 모두 장치 작업자가
`AudioDeviceCreateIOProcID → TellServerAboutStreamUsage → SetPropertyData`의
동기 Mach 응답을 기다린다. macos-26-arm64에서는 354–369행이다.
엔진 로그는 `CoreAudio connection starting` 뒤 5초 후 초기화 실패를 기록한다.

HAL의 `set_stream_active`는 같은 값을 다시 받아도 STREAM_ACTIVE 알림을 보냈다.
호스트가 알림을 받아 현재 stream usage를 다시 적용하면 같은 알림이 반복된다.
수정은 원자적 swap으로 실제 전이만 알리고, 재진입 시 같은 값은 성공으로 반환한다.
회귀 테스트는 호스트가 알림 중 현재 usage를 다시 쓰게 하고, 기본값 재설정에는 알림이
없으며 실제 전이에는 알림이 한 번씩 오는지 확인한다. 수정 전 실패, 수정 후 통과했다.
타임아웃과 오디오 assertion은 변경하지 않았다.

서버측 HAL 호스트의 stack은 원본에 없다. 재알림 결함은 로컬에서 재현했지만,
이 수정으로 hosted IOProc 초기화가 끝나는지는 새 설치본으로 확인해야 한다.

## GRAPH-01: 테스트 도구의 runner 권한 누락

`installed-target-macos-15-x64/fresh/live-coreaudio-system.log:8809` 이후의
TCC subject는 테스트 번들 ID가 아닌 `io.mixrelay.qualification.ui.xctrunner`다.
같은 파일 8905행 이후에는 이 runner의 Accessibility 요청이 기록돼 있다.
`fresh/installed-ui.log`는 해당 runner를 실행한 뒤 AXIsProcessTrusted 검사에서
실패한 것을 보여 준다. 기존 스크립트에는 이 생성 runner에 권한을 주는 단계가 없다.

도구는 build-for-testing 후 생성 runner를 ad-hoc 서명하고, 서명된 코드의 designated
requirement를 csreq로 변환해 일회용 hosted 시스템 TCC DB에 등록한다. tccd 캐시를
재시작한 뒤 test-without-building으로 같은 runner를 실행한다. 병렬 runner 복제를
끄고, 실행 중인 번들 ID·경로·PID·AX 신뢰 상태를 증거로 남겨 예상 신원과 비교한다.
설치된 제품 바이너리는 수정하지 않는다. 오디오 권한 관찰과 그래프 assertion은 유지한다.
순서·신원·권한 등록 코드 요구사항은 Node 테스트로 검증했다. 실제 hosted TCC 승인은
아직 실행하지 않았다.

## macos-26-x64 fresh PKG-02: 제품의 IconServices 대기

`installed-target-macos-26-x64/fresh/shutdown-app-log-driver-runtime-47033.log`의
마지막 drain 단계는 `local_socket`이다. 35초 후 controls 종료 미확인으로 실패한다.
`fresh/shutdown-desktop-47033.sample.txt`와 최상위 `desktop-47033.sample.txt`에는
두 Tokio 작업자가 `NSImage CGImageForProposedRect → ISConcreteIcon →
xpc_connection_send_message_with_reply_sync`에서 기다리는 모습이 있다.
따라서 엔진 종료 단계에 도달하기 전에 앱 목록의 아이콘 조회가 제어 drain을 막았다.

앱 목록은 NSRunningApplication.icon·NSWorkspace.iconForFile 대신 번들에 선언된
아이콘 파일을 읽는다. 앱 목록 요청도 취소 가능한 읽기로 분류했다. 네이티브 회귀
fixture는 두 IconServices 진입점을 예외로 막고 합성 앱의 PNG와 아이콘 부재를
실제 제품 열거 함수로 검증한다. 이전 제품 코드는 application.icon 호출에서 실패하고
수정 코드는 통과한다. 소켓 테스트는 미완료 앱 목록 읽기를 Quit이 취소하면서도
수락한 쓰기는 계속 기다리는지 확인한다.

## macos-15-x64 upgrade PKG-01: 후보 stapler 진단 유실

원본 job `120690112562.log:259`와 `upgrade/PKG-01.failure.json`의 실패 경로는
`candidate/MixRelay-Installer-0.1.0.pkg`다. baseline 경로가 아니다.
`upgrade/baseline/package-install.log`, `baseline/installed-binaries.json`,
`upgrade/upgrade-state-before.json`에는 baseline 설치와 상태 작성 완료 증거가 있다.
같은 job의 fresh PKG-01은 동일 후보 PKG의 stapler 검증을 통과했다.

stapler의 stdout·stderr·종료 코드는 기존 도구가 버렸으므로 원본 로그만으로 네트워크
장애와 티켓 결함을 구별할 수 없다. 재시도는 추가하지 않았다. 패키지 사전 검증의
명령·인자·stdout·stderr·종료 상태를 `package-verification.json`에 보존하고 실패 메시지에도
원시 출력을 포함한다. 회귀 테스트는 stapler 실패를 보존하고 추출·설치에 진입하지
않는지 확인한다. 원래 실패의 분류는 원시 진단을 갖춘 재실행까지 미확정이다.

## baseline 선택

실제 baseline `8582d158455c56006e4288146dcc646100c34bb5`의 PKG를 추출한 HAL에는
NullAudio와 같은 factory UUID `99A15A8B-DA3C-42C3-BD5D-D035A0C2377A`가 있다.
원시는 `/tmp/mr-e2e-r3/baseline-hal-metadata.txt`에 있다. 이 제품 결함은
`55b5c8e5`에서 이미 수정됐으며 harness 예외로 처리하지 않는다.

`qa-install-8582d15`는 정상 HAL 기준선에서 교체해야 한다. 최소 소스 조건은
`55b5c8e5` 포함이다. 보유한 `05f7ac86`도 그 조건은 충족하지만 이번 IOProc·Quit 결함이
있으므로, 두 수정이 포함되고 fresh 설치·오디오·Quit을 통과한 후보를 새 baseline으로
고정한 뒤 그 다음 후보와 업그레이드를 검증하는 것을 권한다. workflow 입력이나 기존
baseline 파일은 이번 작업에서 변경하지 않았다.

## 로컬 검증

- `node --test scripts/e2e/*.test.mjs`: 92개 통과. 새 네이티브 아이콘 fixture와 runner
  신원·권한·실행 순서, stapler 진단 보존 검사를 포함한다.
- 전체 `node --test scripts/*.test.mjs scripts/lab/*.test.mjs scripts/e2e/*.test.mjs`:
  492개 통과, 3개 실패, 기존 Linux ELF 전용 검사 1개 미실행. 실패는 pkgbuild의 분석
  plist 쓰기 거부, Installer 접근, AuthorizationCreate 거부다. 원시는
  `/tmp/mr-e2e-r3/scripts-tests-final.log`에 있다.
- `cargo test -p mixrelay-desktop --bins`: 226개 통과.
- `cargo test -p mixrelay-process-tap --lib`: 57개 통과.
- `cargo test -p mixrelay-macos-test-support`: 네이티브 ABI 검사 7개 통과.
- `cargo test -p mixrelay-hal-core --lib`: 91개 통과, 22개 실패. 모두 기존 네이티브
  fixture의 `create_shared_memory(size, true)`가 sandbox에서 EPERM을 반환한 경우다.
  `/tmp/mr-e2e-r3/hal-tests.log`에 원인이 남아 있다. 새 stream usage 회귀는 수정 전
  실패했고 수정 후 독립 실행과 전체 실행에서 통과했다.
- touched crate의 `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all --check`,
  `git diff --check` 통과. Desktop은 `scripts/lab/native-check.mjs`의 code-check 환경으로
  검사했다. Swift XCTest 소스도 Xcode의 XCTest overlay를 포함한 타입 검사를 통과했다.

로컬 Cargo는 기본 sccache daemon의 worktree 쓰기 거부를 피해 `RUSTC_WRAPPER=`와
`CARGO_TARGET_DIR=/private/tmp/mr-e2e-r3-target`로 실행했다. 코드와 assertion은 그 제약에
맞춰 바꾸지 않았다. 새 서명 PKG, 여섯 hosted 대상, 실제 TCC 승인, UI 조작, native
오디오·Quit의 설치본 검증은 실행하지 않았다. 원래 stapler 실패의 네트워크·티켓 분류도
미확정이다. 커밋·push·workflow dispatch는 수행하지 않았다.
