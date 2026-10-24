# macOS hosted 설치본 실패 조사: 950ff3ac

대상 실행은 `40019418055`다. 원시 증거의 루트는
`/tmp/mr-e2e.Cqk3/qualification/installed-target-<target>/`이며,
아래 `fresh/` 경로는 각 대상 폴더를 기준으로 한다.
이 문서는 소스 수정과 기존 실행의 진단을 기록한다. 수정한 설치본의 통과 기록은 아니다.

| 항목 | 확인한 원인과 분류 | 수정 |
| --- | --- | --- |
| AUDIO-01 | 테스트의 첫 권한 관찰 실패. 14는 UI 프로젝트 빌드 실패로 관찰기가 없고, 나머지 다섯 대상은 GUI 앱이 아닌 `com.apple.tccd`에 대한 XCTest 조회가 예외를 일으켜 관찰기가 종료된다. 이후 엔진의 마이크 권한 요청이 응답 없이 남고 IOProc 등록이 대기한다. | 실제 실행 중인 GUI 권한 창 소유자만 PID로 조회한다. 관찰기를 Quit·제거 시도까지 유지한다. |
| GRAPH-01 | 테스트 도구 결함. 편집기와 런타임이 같은 번들 ID를 쓰며, XCTest의 비공개 `setProcessID:`로 선택한 PID가 후속 조회에 유지되지 않는다. 14는 Xcode 15.4가 프로젝트 형식 77을 읽지 못한다. | 공개 AX API의 프로세스별 루트로 편집기만 조작하고 설치 경로·역할·창을 확인한다. XcodeGen 형식을 60, Xcode 버전을 15.4로 고정한다. |
| PKG-02, 26 x64 | Quit은 처리됐다. 런타임이 제어 요청 배출에 실패해 오디오 소유권을 유지했으며, 설치 도우미의 정상 Quit과 SIGTERM 시도도 완료되지 않았다. 정확히 어느 요청이 남았는지는 기존 증거에 없다. 테스트는 권한 관찰기를 Quit 전에 종료하는 순서 결함도 갖고 있다. 제품에서는 관찰용 Invoke 요청도 취소 불가능한 mutation으로 분류한다. | 관찰기를 제거까지 유지한다. 부작용 없는 관찰 요청은 연결 종료·Quit 때 취소하고, 승인된 쓰기와 캡처 소유권을 조정하는 읽기는 완료까지 기다린다. 종료 단계 로그와 Quit 실패 직후 런타임 스택을 추가한다. |

오디오 증거는 여섯 대상 모두 동일한 방향을 가리킨다.

- `fresh/live-coreaudio-devices.txt`와 `fresh/live-coreaudio-probe.txt`에 MixRelay IO가 게시돼 있다.
  물리 장치가 없는 VM에서도 가상 장치 자체를 시험할 수 있다.
- `fresh/live-engine.sample.txt`의 `mixrelay-coreaudio-device` 스레드는
  `AudioDeviceCreateIOProcID` → `_TellServerAboutStreamUsage` → `mach_msg`에서 대기한다.
- `fresh/live-coreaudio-system.log`에는 엔진에 대한
  `Refusing TCCAccessRequest ... in background session`과 뒤이은
  `AUTHREQ_PROMPTING ... service=kTCCServiceMicrophone`가 있다.
  예를 들어 15 arm64에서는 20:32:22.716과 20:32:22.843 UTC다.
  관찰기가 종료되기 전 해당 요청의 완료 응답은 수집 로그에 없다.
- 다섯 대상의 `fresh/installed-ui.log`에는
  `The app representing com.apple.tccd could not be found. (NSInternalInconsistencyException)`가 있다.
  15 arm64에서는 Swift 66행 예외 뒤 23.42초에 테스트가 종료된다.
- 14 arm64의 `fresh/installed-ui.log` 16–17행에는
  `future Xcode project file format (77)` 오류가 있다.
  형식은 XcodeGen의 [`projectFormat`](https://github.com/yonaskolb/XcodeGen/blob/master/Docs/ProjectSpec.md)
  옵션으로 지정한다. `xcodeVersion`은 프로젝트 형식을 바꾸는 옵션이 아니다.

이 증거로 AUDIO-01을 불가피한 hosted VM 제약으로 면제할 수 없다.
첫 권한 승인이 끝난 뒤의 PCM 전달은 기존 실행에서 확인되지 않았다.
엔진의 5초 초기화 제한을 늘리거나, 가짜 backend로 바꾸거나, 필수 시나리오를 생략하지 않는다.

별도의 제품 결함도 확인했다. 여섯 대상의 `fresh/live-coreaudio-system.log`에
`More than one bundle with the same factory UUID detected`가 있고,
`NullAudio.driver`와 `MixRelayHAL.driver`가 모두
`99A15A8B-DA3C-42C3-BD5D-D035A0C2377A`를 등록한다.
MixRelay의 factory UUID를 고유한 값으로 바꾸고 Apple의 플러그인 type UUID는 유지했다.
회귀 테스트는 NullAudio의 실제 factory ID와 함께 등록했을 때 MixRelay의 type이
자기 factory를 가리키는지 검사한다. 이 충돌이 위 시간 초과의 유일한 원인이라는 증거는 없다.

GRAPH-01의 PID 불일치는 15 arm64의 `fresh/installed-ui.error`에서 직접 확인된다.
요청한 편집기는 PID 2920인데 XCTest 트리의 Application은 PID 2934이며,
창 대신 런타임의 상태 메뉴가 나온다. 다른 다섯 대상을 같은 방식으로 통과했다고 간주하지 않는다.

26 x64의 `fresh/shutdown.log`와 `fresh/PKG-02.failure.json`에는 PID 2498의 Quit 실패가 있다.
같은 대상의 `app-log-driver-runtime-2498.log`에는 20:32:47, 20:34:27,
20:36:39 UTC에 `Controls are still finishing; audio ownership was retained.`가 기록돼 있다.
따라서 Quit 이벤트가 무시됐다고 설명할 수 없다. 새 소켓 회귀 테스트는 주입한 대기 중
GET 때문에 종료가 막히는 결함을 수정 전 분류에서 재현하고, 수정 후에는 GET 취소와
PUT 완료 대기를 함께 검사한다. 기존 실행의 정확한 대기 명령을 재현했다는 뜻은 아니다.

검증 범위와 남은 확인:

- `RUSTC_WRAPPER= cargo test -p mixrelay-desktop --bin mixrelay-desktop`: 226개 통과.
- `RUSTC_WRAPPER= cargo test -p mixrelay-hal-bundle --bins`: 7개 통과.
  실제 서명 호출은 실행하지 않는 도구 단위 테스트다.
- `node --test scripts/e2e/*.test.mjs`: 66개 통과, skip 0.
- `RUSTC_WRAPPER= cargo clippy -p mixrelay-desktop --all-targets --all-features`와
  HAL core·bundle의 같은 clippy 검사: workspace lint로 통과.
  최초 desktop clippy는 `frontendDist` 부재로 실패했고,
  기존 의존성으로 `pnpm --filter @mixrelay/control-ui build`를 실행한 뒤 통과했다.
- Xcode 27.1의 `swiftc`로 UI 테스트 소스 전체를 macOS 14.2 대상
  arm64·x86_64 라이브러리로 빌드했다. arm64 `xctest`에서
  `InstalledApplicationTests` 3개가 통과했다.
  로컬에 XcodeGen이 없어 프로젝트 생성 및 그 프로젝트의 `xcodebuild test`는 실행하지 못했다.
- `RUSTC_WRAPPER= cargo test -p mixrelay-hal-core --lib`: 90개 통과, 22개 실패.
  실패는 모두 기존 shared-memory fixture의
  `native/macos/hal-driver/rust/src/native_tests.rs:112`에서
  `create_shared_memory`가 sandbox의 `EPERM`으로 거부된 결과다.
  테스트를 제외하거나 sandbox 우회를 시도하지 않았다.
- `cargo fmt --all -- --check`, `bash -n scripts/e2e/macos-ui.sh`,
  `plutil -lint native/macos/hal-driver/driver/Info.plist`, `git diff --check`는 통과했다.
- AX 접근성 신뢰가 없는 UI runner는 명시적으로 실패한다. 권한 데이터베이스를 변경하지 않는다.
  hosted XCTest runner의 AX 접근과 실제 창·메뉴·권한 클릭은 CI 확인이 필요하다.
- 제품 factory 메타데이터가 바뀌므로 새 소스의 서명·공증된 후보가 필요하다.
  여섯 hosted 대상의 fresh·upgrade 전체 시나리오와 제거를 다시 실행해야 한다.
  관찰된 macOS 14 patch는 최소 14.2 양쪽 CPU의 기존 검증 공백을 대체하지 않는다.
- 이 조사에서는 설치, 서명, 공증, 키체인 변경, workflow dispatch, push, commit을 하지 않았다.
